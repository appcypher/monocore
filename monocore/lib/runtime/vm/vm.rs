use std::{ffi::CString, path::PathBuf};

use getset::Getters;
use typed_path::Utf8UnixPathBuf;

use crate::{
    config::{PathPair, PortPair},
    runtime::ffi,
    utils, InvalidMicroVMConfigError, MonocoreError, MonocoreResult,
};

use super::{EnvPair, LinuxRlimit, MicroVMBuilder};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// A lightweight Linux virtual machine.
#[derive(Debug, Getters)]
pub struct MicroVM {
    /// The context ID for the microVM.
    ctx_id: u32,

    /// The configuration for the microVM.
    #[get = "pub with_prefix"]
    config: MicroVMConfig,
}

/// A configuration for a microVM.
#[derive(Debug)]
pub struct MicroVMConfig {
    /// The log level to use for the microVM.
    pub log_level: LogLevel,

    /// The path to the root directory for the microVM.
    pub root_path: PathBuf,

    /// The number of vCPUs to use for the microVM.
    pub num_vcpus: u8,

    /// The amount of RAM in MiB to use for the microVM.
    pub ram_mib: u32,

    /// The virtio-fs mounts to use for the microVM.
    pub virtiofs: Vec<PathPair>,

    /// The port map to use for the microVM.
    pub port_map: Vec<PortPair>,

    /// The resource limits to use for the microVM.
    pub rlimits: Vec<LinuxRlimit>,

    /// The working directory path to use for the microVM.
    pub workdir_path: Option<Utf8UnixPathBuf>,

    /// The executable path to use for the microVM.
    pub exec_path: Option<Utf8UnixPathBuf>,

    /// The arguments to pass to the executable.
    pub argv: Vec<String>,

    /// The environment variables to set for the executable.
    pub env: Vec<EnvPair>,

    /// The console output path to use for the microVM.
    pub console_output: Option<Utf8UnixPathBuf>,
}

/// The log level to use for the microVM.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum LogLevel {
    /// No logging.
    #[default]
    Off = 0,

    /// Error messages.
    Error = 1,

    /// Warning messages.
    Warn = 2,

    /// Informational messages.
    Info = 3,

    /// Debug messages.
    Debug = 4,

    /// Trace messages.
    Trace = 5,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl MicroVM {
    /// Creates a new microVM from a configuration.
    pub fn from_config(config: MicroVMConfig) -> MonocoreResult<Self> {
        let ctx_id = Self::create_ctx();

        config.validate()?;

        Self::apply_config(ctx_id, &config);

        Ok(Self { ctx_id, config })
    }

    /// Creates a builder for a microVM.
    pub fn builder() -> MicroVMBuilder<(), ()> {
        MicroVMBuilder::default()
    }

    /// Starts the microVM.
    pub fn start(&self) {
        Self::start_vm(self.ctx_id)
    }

    fn create_ctx() -> u32 {
        let ctx_id = unsafe { ffi::krun_create_ctx() };
        assert!(ctx_id >= 0, "Failed to create microVM context: {}", ctx_id);
        ctx_id as u32
    }

    fn apply_config(ctx_id: u32, config: &MicroVMConfig) {
        // Set log level
        unsafe {
            let status = ffi::krun_set_log_level(config.log_level as u32);
            assert!(status >= 0, "Failed to set log level: {}", status);
        }

        // Set basic VM configuration
        unsafe {
            let status = ffi::krun_set_vm_config(ctx_id, config.num_vcpus, config.ram_mib);
            assert!(status >= 0, "Failed to set VM config: {}", status);
        }

        // Set root path
        let c_root_path = CString::new(config.root_path.to_str().unwrap().as_bytes()).unwrap();
        unsafe {
            let status = ffi::krun_set_root(ctx_id, c_root_path.as_ptr());
            assert!(status >= 0, "Failed to set root path: {}", status);
        }

        // Add virtio-fs mounts
        for mount in &config.virtiofs {
            let tag = CString::new(mount.get_guest().to_string().as_bytes()).unwrap();
            let path = CString::new(mount.get_host().to_string().as_bytes()).unwrap();
            unsafe {
                let status = ffi::krun_add_virtiofs(ctx_id, tag.as_ptr(), path.as_ptr());
                assert!(status >= 0, "Failed to add virtio-fs mount: {}", status);
            }
        }

        // Set port map
        let c_port_map: Vec<_> = config
            .port_map
            .iter()
            .map(|p| CString::new(p.to_string()).unwrap())
            .collect();
        let c_port_map_ptrs = utils::to_null_terminated_c_array(&c_port_map);

        unsafe {
            let status = ffi::krun_set_port_map(ctx_id, c_port_map_ptrs.as_ptr());
            assert!(status >= 0, "Failed to set port map: {}", status);
        }

        // Set resource limits
        if !config.rlimits.is_empty() {
            let c_rlimits: Vec<_> = config
                .rlimits
                .iter()
                .map(|s| CString::new(s.to_string()).unwrap())
                .collect();
            let c_rlimits_ptrs = utils::to_null_terminated_c_array(&c_rlimits);
            unsafe {
                let status = ffi::krun_set_rlimits(ctx_id, c_rlimits_ptrs.as_ptr());
                assert!(status >= 0, "Failed to set resource limits: {}", status);
            }
        }

        // Set working directory
        if let Some(workdir) = &config.workdir_path {
            let c_workdir = CString::new(workdir.to_string().as_bytes()).unwrap();
            unsafe {
                let status = ffi::krun_set_workdir(ctx_id, c_workdir.as_ptr());
                assert!(status >= 0, "Failed to set working directory: {}", status);
            }
        }

        // Set executable path, arguments, and environment variables
        if let Some(exec_path) = &config.exec_path {
            let c_exec_path = CString::new(exec_path.to_string().as_bytes()).unwrap();

            let c_argv: Vec<_> = config
                .argv
                .iter()
                .map(|s| CString::new(s.as_str()).unwrap())
                .collect();
            let c_argv_ptrs = utils::to_null_terminated_c_array(&c_argv);

            let c_env: Vec<_> = config
                .env
                .iter()
                .map(|s| CString::new(s.to_string()).unwrap())
                .collect();
            let c_env_ptrs = utils::to_null_terminated_c_array(&c_env);

            unsafe {
                let status = ffi::krun_set_exec(
                    ctx_id,
                    c_exec_path.as_ptr(),
                    c_argv_ptrs.as_ptr(),
                    c_env_ptrs.as_ptr(),
                );
                assert!(
                    status >= 0,
                    "Failed to set executable configuration: {}",
                    status
                );
            }
        } else {
            // If no executable path is set, we still need to set the environment variables
            let c_env: Vec<_> = config
                .env
                .iter()
                .map(|s| CString::new(s.to_string()).unwrap())
                .collect();
            let c_env_ptrs = utils::to_null_terminated_c_array(&c_env);

            unsafe {
                let status = ffi::krun_set_env(ctx_id, c_env_ptrs.as_ptr());
                assert!(
                    status >= 0,
                    "Failed to set environment variables: {}",
                    status
                );
            }
        }

        // Set console output
        if let Some(console_output) = &config.console_output {
            let c_console_output = CString::new(console_output.to_string().as_bytes()).unwrap();
            unsafe {
                let status = ffi::krun_set_console_output(ctx_id, c_console_output.as_ptr());
                assert!(status >= 0, "Failed to set console output: {}", status);
            }
        }
    }

    fn start_vm(ctx_id: u32) {
        unsafe {
            let status = ffi::krun_start_enter(ctx_id);
            assert!(status >= 0, "Failed to start microVM: {}", status);
        }
    }
}

impl MicroVMConfig {
    /// Validates the microVM configuration.
    pub fn validate(&self) -> MonocoreResult<()> {
        if !self.root_path.exists() {
            return Err(MonocoreError::InvalidMicroVMConfig(
                InvalidMicroVMConfigError::RootPathDoesNotExist(
                    self.root_path.to_str().unwrap().into(),
                ),
            ));
        }

        if self.num_vcpus == 0 {
            return Err(MonocoreError::InvalidMicroVMConfig(
                InvalidMicroVMConfigError::NumVCPUsIsZero,
            ));
        }

        if self.ram_mib == 0 {
            return Err(MonocoreError::InvalidMicroVMConfig(
                InvalidMicroVMConfigError::RamIsZero,
            ));
        }

        Ok(())
    }
}

//--------------------------------------------------------------------------------------------------
// Trait Implementations
//--------------------------------------------------------------------------------------------------

impl Drop for MicroVM {
    fn drop(&mut self) {
        unsafe { ffi::krun_free_ctx(self.ctx_id) };
    }
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------
