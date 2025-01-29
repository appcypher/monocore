//! `mfsrun` is a polymorphic binary that can operate in two modes: NFS server or supervisor.
//!
//! # Overview
//!
//! This binary provides a unified interface for running either:
//! - An NFS server that serves a monofs filesystem
//! - A supervisor process that can manage and monitor child processes
//!
//! ## Usage
//!
//! ### NFS Server Mode
//!
//! To run as an NFS server:
//! ```bash
//! mfsrun nfsserver \
//!     --host=127.0.0.1 \
//!     --port=2049 \
//!     --store-path=/path/to/store
//! ```
//!
//! #### NFS Server Parameters
//!
//! - `--host`: The address to bind to (default: "127.0.0.1")
//! - `--port`: The port to listen on (default: 2049)
//! - `--store-path`: Directory path where the monofs store will be located
//!
//! ### Supervisor Mode
//!
//! To run as a supervisor:
//! ```bash
//! mfsrun supervisor \
//!     --log-dir=/path/to/logs \
//!     --child-name=my_fs \
//!     --child-log-prefix=mfsrun \
//!     --host=127.0.0.1 \
//!     --port=2049 \
//!     --store-dir=/path/to/store \
//!     --db-path=/path/to/mfsrun.db
//! ```
//!
//! ### Supervisor Parameters
//!
//! - `--log-dir`: Directory where log files will be stored
//! - `--child-name`: Name to identify the supervised process
//! - `--host`: The address for the NFS server to bind to (default: "127.0.0.1")
//! - `--port`: The port for the NFS server to listen on (default: 2049)
//! - `--store-dir`: Directory path where the monofs store will be located
//! - `--db-path`: Path to the metrics database file
//!
//! ## Examples
//!
//! ### Running an NFS Server with Custom Port
//! ```bash
//! mfsrun nfsserver \
//!     --host=0.0.0.0 \
//!     --port=2050 \
//!     --store-path=/mnt/monofs/store
//! ```
//!
//! ### Supervising an NFS Server Process
//! ```bash
//! mfsrun supervisor \
//!     --log-dir=/var/log/monofs \
//!     --child-name=my_fs \
//!     --child-log-prefix=mfsrun \
//!     --host=0.0.0.0 \
//!     --port=2049 \
//!     --store-path=/mnt/monofs/store \
//!     --db-path=/path/to/mfsrun.db
//! ```
//!
//! Note: When running in supervisor mode, the supervisor will automatically use the current
//! executable as the child process, allowing for self-supervision of the NFS server.

use std::env;

use anyhow::Result;
use clap::Parser;
use monofs::{
    cli::{MfsRuntimeArgs, MfsRuntimeSubcommand},
    runtime::MfsRuntimeMetricsMonitor,
    server::MonofsServer,
    utils::path::MFSRUN_LOG_PREFIX,
};
use monoutils::runtime::Supervisor;

//--------------------------------------------------------------------------------------------------
// Functions: main
//--------------------------------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let args = MfsRuntimeArgs::parse();

    match args.subcommand {
        MfsRuntimeSubcommand::Nfsserver {
            host,
            port,
            store_dir,
        } => {
            // Create and start NFS server
            let server = MonofsServer::new(store_dir, host, port);
            tracing::info!(
                "Starting NFS server on {}:{}",
                server.get_host(),
                server.get_port()
            );
            tracing::info!("Using store at: {}", server.get_store_dir().display());

            server.start().await?;
        }
        MfsRuntimeSubcommand::Supervisor {
            log_dir,
            child_name,
            host,
            port,
            store_dir,
            db_path,
        } => {
            // Get current executable path
            let current_exe = env::current_exe()?;

            // Create metrics monitor
            let metrics_monitor = MfsRuntimeMetricsMonitor::new(db_path);

            // Compose child arguments
            let child_args = vec![
                "nfsserver".to_string(),
                format!("--host={}", host),
                format!("--port={}", port),
                format!("--store-dir={}", store_dir.display()),
            ];

            // Create and start supervisor
            let mut supervisor = Supervisor::new(
                current_exe,
                child_args,
                child_name,
                MFSRUN_LOG_PREFIX,
                log_dir,
                metrics_monitor,
            );

            supervisor.start().await?;
        }
    }

    Ok(())
}
