//! To run this example:
//! ```bash
//! make example microvm_nop
//! ```

use monocore::{utils, vm::MicroVm};

//--------------------------------------------------------------------------------------------------
// Functions: main
//--------------------------------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    // Use specific directories for OCI and rootfs
    let build_dir = format!("{}/build", env!("CARGO_MANIFEST_DIR"));
    let oci_dir = format!("{}/oci", build_dir);
    let rootfs_alpine_dir = format!("{}/rootfs/reference/library_alpine__latest", build_dir);

    // Pull and merge Alpine image
    utils::pull_docker_image(&oci_dir, "library/alpine:latest").await?;
    utils::merge_image_layers(&oci_dir, &rootfs_alpine_dir, "library/alpine:latest").await?;

    // Build the MicroVm
    let vm = MicroVm::builder()
        .root_path(format!("{}/merged", rootfs_alpine_dir))
        .exec_path("/bin/true")
        .ram_mib(1024)
        .build()?;

    // Start the MicroVm
    tracing::info!("Starting MicroVm...");
    vm.start()?;

    Ok(())
}
