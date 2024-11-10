//! If you are trying to run this example, please make sure to run `make example microvm_nop` from
//! the `monocore` subdirectory

use monocore::vm::MicroVm;

//--------------------------------------------------------------------------------------------------
// Functions: main
//--------------------------------------------------------------------------------------------------

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    // Use the architecture-specific build directory
    let rootfs_path = format!(
        "{}/build/rootfs-alpine-{}",
        env!("CARGO_MANIFEST_DIR"),
        get_current_arch()
    );

    // Build the MicroVm
    let vm = MicroVm::builder()
        .root_path(&rootfs_path)
        .exec_path("/bin/true")
        .ram_mib(1024)
        .build()?;

    // Start the MicroVm
    tracing::info!("Starting MicroVm...");
    vm.start()?;

    Ok(())
}

//--------------------------------------------------------------------------------------------------
// Functions: *
//--------------------------------------------------------------------------------------------------

// Add this function to determine the current architecture
fn get_current_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        panic!("Unsupported architecture")
    }
}
