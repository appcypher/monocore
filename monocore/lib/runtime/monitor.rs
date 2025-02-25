use std::{
    io::{Read, Write},
    os::fd::BorrowedFd,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use monoutils::{
    ChildIo, MonoutilsError, MonoutilsResult, ProcessMonitor, RotatingLog, LOG_SUFFIX,
};
use sqlx::{Pool, Sqlite};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
};

use crate::{management, utils::MCRUN_LOG_PREFIX, MonocoreResult};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// A process monitor for MicroVMs
pub struct MicroVmMonitor {
    /// The database for tracking sandbox metrics and metadata
    sandbox_db: Pool<Sqlite>,

    /// The supervisor PID
    supervisor_pid: u32,

    /// The MicroVM log path
    log_path: Option<PathBuf>,

    /// The log directory
    log_dir: PathBuf,

    /// The root filesystem path
    root_path: PathBuf,

    /// The retention duration for log files
    retention_duration: Duration,

    /// original terminal settings for STDIN (set in TTY mode)
    original_term: Option<nix::sys::termios::Termios>,

    /// Whether to forward output to stdout/stderr
    forward_output: bool,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl MicroVmMonitor {
    /// Create a new MicroVM monitor
    pub async fn new(
        supervisor_pid: u32,
        sandbox_db_path: impl AsRef<Path>,
        log_dir: impl Into<PathBuf>,
        root_path: impl Into<PathBuf>,
        retention_duration: Duration,
        forward_output: bool,
    ) -> MonocoreResult<Self> {
        Ok(Self {
            supervisor_pid,
            sandbox_db: management::get_db_pool(sandbox_db_path.as_ref()).await?,
            log_path: None,
            log_dir: log_dir.into(),
            root_path: root_path.into(),
            retention_duration,
            original_term: None,
            forward_output,
        })
    }

    /// Generates a unique log name using name, process ID, and current timestamp.
    ///
    /// The ID format is: "mcrun-{name}-{timestamp}-{child_pid}.log"
    fn generate_log_name(&self, child_pid: u32, name: impl AsRef<str>) -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        format!(
            "{}-{}-{}-{}.{}",
            MCRUN_LOG_PREFIX,
            name.as_ref(),
            timestamp,
            child_pid,
            LOG_SUFFIX
        )
    }

    fn restore_terminal_settings(&mut self) {
        if let Some(original_term) = self.original_term.take() {
            if let Err(e) = nix::sys::termios::tcsetattr(
                unsafe { BorrowedFd::borrow_raw(libc::STDIN_FILENO) },
                nix::sys::termios::SetArg::TCSANOW,
                &original_term,
            ) {
                tracing::warn!(error = %e, "failed to restore terminal settings in restore_terminal_settings");
            }
        }
    }
}

//--------------------------------------------------------------------------------------------------
// Trait Implementations
//--------------------------------------------------------------------------------------------------

#[async_trait]
impl ProcessMonitor for MicroVmMonitor {
    async fn start(&mut self, pid: u32, name: String, child_io: ChildIo) -> MonoutilsResult<()> {
        let log_name = self.generate_log_name(pid, &name);
        let log_path = self.log_dir.join(&log_name);

        let microvm_log =
            std::sync::Arc::new(tokio::sync::Mutex::new(RotatingLog::new(&log_path).await?));
        let microvm_pid = pid;

        self.log_path = Some(log_path);

        // Insert sandbox entry into database
        sqlx::query(
            r#"
            INSERT INTO sandboxes (name, supervisor_pid, microvm_pid, status, root_path)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(name)
        .bind(self.supervisor_pid)
        .bind(microvm_pid)
        .bind("STARTING")
        .bind(self.root_path.to_string_lossy().into_owned())
        .execute(&self.sandbox_db)
        .await
        .map_err(MonoutilsError::custom)?;

        match child_io {
            ChildIo::Piped {
                stdin,
                stdout,
                stderr,
                ..
            } => {
                // Handle stdout logging
                if let Some(mut stdout) = stdout {
                    let log = microvm_log.clone();
                    let forward_output = self.forward_output;
                    tokio::spawn(async move {
                        let mut buf = [0u8; 1024];
                        while let Ok(n) = stdout.read(&mut buf).await {
                            if n == 0 {
                                break;
                            }
                            // Write to log file
                            let mut log_guard = log.lock().await;
                            if let Err(e) = log_guard.write_all(&buf[..n]).await {
                                tracing::error!(microvm_pid = microvm_pid, error = %e, "failed to write to microvm stdout log");
                            }
                            if let Err(e) = log_guard.flush().await {
                                tracing::error!(microvm_pid = microvm_pid, error = %e, "failed to flush microvm stdout log");
                            }

                            // Also forward to parent's stdout if enabled
                            if forward_output {
                                print!("{}", String::from_utf8_lossy(&buf[..n]));
                                // Flush stdout in case data is buffered
                                if let Err(e) = std::io::stdout().flush() {
                                    tracing::warn!(error = %e, "failed to flush parent stdout");
                                }
                            }
                        }
                    });
                }

                // Handle stderr logging
                if let Some(mut stderr) = stderr {
                    let log = microvm_log.clone();
                    let forward_output = self.forward_output;
                    tokio::spawn(async move {
                        let mut buf = [0u8; 1024];
                        while let Ok(n) = stderr.read(&mut buf).await {
                            if n == 0 {
                                break;
                            }
                            // Write to log file
                            let mut log_guard = log.lock().await;
                            if let Err(e) = log_guard.write_all(&buf[..n]).await {
                                tracing::error!(microvm_pid = microvm_pid, error = %e, "failed to write to microvm stderr log");
                            }
                            if let Err(e) = log_guard.flush().await {
                                tracing::error!(microvm_pid = microvm_pid, error = %e, "failed to flush microvm stderr log");
                            }

                            // Also forward to parent's stderr if enabled
                            if forward_output {
                                eprint!("{}", String::from_utf8_lossy(&buf[..n]));
                                // Flush stderr in case data is buffered
                                if let Err(e) = std::io::stderr().flush() {
                                    tracing::warn!(error = %e, "failed to flush parent stderr");
                                }
                            }
                        }
                    });
                }

                // Handle stdin streaming from parent to child
                if let Some(mut child_stdin) = stdin {
                    tokio::spawn(async move {
                        let mut parent_stdin = tokio::io::stdin();
                        if let Err(e) = tokio::io::copy(&mut parent_stdin, &mut child_stdin).await {
                            tracing::warn!(error = %e, "failed to copy parent stdin to child stdin");
                        }
                    });
                }
            }
            ChildIo::TTY {
                master_read,
                mut master_write,
            } => {
                // Handle TTY I/O
                // Put terminal in raw mode
                let term = nix::sys::termios::tcgetattr(unsafe {
                    BorrowedFd::borrow_raw(libc::STDIN_FILENO)
                })?;
                self.original_term = Some(term.clone());
                let mut raw_term = term.clone();
                nix::sys::termios::cfmakeraw(&mut raw_term);
                nix::sys::termios::tcsetattr(
                    unsafe { BorrowedFd::borrow_raw(libc::STDIN_FILENO) },
                    nix::sys::termios::SetArg::TCSANOW,
                    &raw_term,
                )?;

                // Spawn async task to read from the master
                let log = microvm_log.clone();
                let forward_output = self.forward_output;
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    loop {
                        let mut read_guard = match master_read.readable().await {
                            Ok(guard) => guard,
                            Err(e) => {
                                tracing::warn!(error = %e, "error waiting for master fd to become readable");
                                break;
                            }
                        };

                        match read_guard.try_io(|inner| inner.get_ref().read(&mut buf)) {
                            Ok(Ok(0)) => break, // EOF reached.
                            Ok(Ok(n)) => {
                                // Write to log file
                                let mut log_guard = log.lock().await;
                                if let Err(e) = log_guard.write_all(&buf[..n]).await {
                                    tracing::error!(microvm_pid = microvm_pid, error = %e, "failed to write to microvm tty log");
                                }
                                if let Err(e) = log_guard.flush().await {
                                    tracing::error!(microvm_pid = microvm_pid, error = %e, "failed to flush microvm tty log");
                                }

                                // Print the output from the child process if enabled
                                if forward_output {
                                    print!("{}", String::from_utf8_lossy(&buf[..n]));
                                    // flush stdout in case data is buffered
                                    std::io::stdout().flush().ok();
                                }
                            }
                            Ok(Err(e)) => {
                                tracing::warn!(error = %e, "error reading from master fd");
                                break;
                            }
                            Err(_) => continue,
                        }
                    }
                });

                // Spawn async task to copy parent's stdin to the master
                tokio::spawn(async move {
                    let mut stdin = tokio::io::stdin();
                    if let Err(e) = tokio::io::copy(&mut stdin, &mut master_write).await {
                        tracing::warn!(error = %e, "error copying stdin to master fd");
                    }
                });
            }
        }

        Ok(())
    }

    async fn stop(&mut self) -> MonoutilsResult<()> {
        // Restore terminal settings if they were modified
        self.restore_terminal_settings();

        // Remove sandbox entry from database
        sqlx::query(
            r#"
            DELETE FROM sandboxes
            WHERE supervisor_pid = ?
            "#,
        )
        .bind(self.supervisor_pid)
        .execute(&self.sandbox_db)
        .await
        .map_err(MonoutilsError::custom)?;

        // Batch delete old log files in the log directory
        let now: DateTime<Utc> = Utc::now();
        match fs::read_dir(&self.log_dir).await {
            Ok(mut dir) => {
                while let Some(entry) = dir.next_entry().await? {
                    let path = entry.path();
                    if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
                        if file_name.starts_with(MCRUN_LOG_PREFIX)
                            && file_name.ends_with(LOG_SUFFIX)
                        {
                            let metadata = fs::metadata(&path).await?;
                            if let Ok(modified) =
                                metadata.modified().map(|t| DateTime::<Utc>::from(t))
                            {
                                if now - modified > self.retention_duration {
                                    if let Err(e) = fs::remove_file(&path).await {
                                        tracing::warn!(error = %e, "failed to delete old log file: {:?}", &path);
                                    } else {
                                        tracing::info!("deleted old log file: {:?}", path);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to read log directory");
            }
        }

        // Reset the log path
        self.log_path = None;

        Ok(())
    }
}

impl Drop for MicroVmMonitor {
    fn drop(&mut self) {
        self.restore_terminal_settings();
    }
}
