use std::{fs, os::unix::fs::PermissionsExt};
use tempfile::tempdir;

use monocore::oci::{
    distribution::{DockerRegistry, OciRegistryPull},
    overlayfs::OverlayFsMerger,
};

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[test_log::test(tokio::test)]
#[ignore = "requires pulling Docker images"]
async fn test_oci_merge_basic_merge() -> anyhow::Result<()> {
    // Create temporary directory for test
    let temp_dir = tempdir()?;
    let registry = DockerRegistry::with_oci_dir(temp_dir.path().to_path_buf());

    // Pull a small image with multiple layers (alpine:latest)
    registry
        .pull_image("library/alpine", Some("latest"))
        .await?;

    // Create merger with temporary destination
    let dest_dir = temp_dir.path().join("merged_test");
    fs::create_dir_all(&dest_dir)?;

    let merger = OverlayFsMerger::new(temp_dir.path(), dest_dir.clone());

    // Merge layers
    merger.merge("library_alpine__latest").await?;

    // Verify basic Alpine Linux directories exist
    let expected_dirs = vec!["bin", "etc", "home", "root", "usr", "var"];

    for dir in &expected_dirs {
        assert!(
            dest_dir.join("merged").join(dir).exists(),
            "Directory {} should exist",
            dir
        );
    }

    // Cleanup
    merger.unmount().await?;
    Ok(())
}

#[test_log::test(tokio::test)]
#[ignore = "requires pulling Docker images"]
async fn test_oci_merge_layer_permissions() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let registry = DockerRegistry::with_oci_dir(temp_dir.path().to_path_buf());

    // Pull nginx image which has specific file permissions
    registry.pull_image("library/nginx", Some("alpine")).await?;

    let dest_dir = temp_dir.path().join("merged_perms_test");
    fs::create_dir_all(&dest_dir)?;

    let merger = OverlayFsMerger::new(temp_dir.path(), dest_dir.clone());

    // Merge layers
    merger.merge("library_nginx__alpine").await?;

    // Verify nginx binary permissions
    let nginx_binary = dest_dir.join("merged/usr/sbin/nginx");
    let metadata = fs::metadata(&nginx_binary)?;
    let mode = metadata.permissions().mode();

    // nginx binary should be executable
    assert!(
        mode & 0o111 != 0,
        "nginx binary should have executable permissions"
    );

    // Cleanup
    merger.unmount().await?;
    Ok(())
}

#[test_log::test(tokio::test)]
#[ignore = "requires pulling Docker images"]
async fn test_oci_merge_merge_cleanup() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let registry = DockerRegistry::with_oci_dir(temp_dir.path().to_path_buf());

    // Pull a small image
    registry
        .pull_image("library/alpine", Some("latest"))
        .await?;

    let dest_dir = temp_dir.path().join("merged_cleanup_test");
    fs::create_dir_all(&dest_dir)?;

    let merger = OverlayFsMerger::new(temp_dir.path(), dest_dir.clone());

    // Merge layers
    merger.merge("library_alpine__latest").await?;

    // Verify merged directory is created
    assert!(dest_dir.join("merged").exists());

    // Unmount and cleanup
    merger.unmount().await?;

    // Verify merged directory is cleaned up
    assert!(!dest_dir.join("merged").exists());

    Ok(())
}

#[test_log::test(tokio::test)]
#[ignore = "requires pulling Docker images"]
async fn test_oci_merge_concurrent_merges() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let registry = DockerRegistry::with_oci_dir(temp_dir.path().to_path_buf());

    // Pull two different images
    let pull_tasks = tokio::join!(
        registry.pull_image("library/alpine", Some("latest")),
        registry.pull_image("library/busybox", Some("latest"))
    );
    pull_tasks.0?;
    pull_tasks.1?;

    // Create two separate merge destinations
    let dest_dir1 = temp_dir.path().join("merged_concurrent_1");
    let dest_dir2 = temp_dir.path().join("merged_concurrent_2");
    fs::create_dir_all(&dest_dir1)?;
    fs::create_dir_all(&dest_dir2)?;

    let merger1 = OverlayFsMerger::new(temp_dir.path(), dest_dir1.clone());
    let merger2 = OverlayFsMerger::new(temp_dir.path(), dest_dir2.clone());

    // Merge concurrently
    let merge_results = tokio::join!(
        merger1.merge("library_alpine__latest"),
        merger2.merge("library_busybox__latest")
    );

    // Check results
    merge_results.0?;
    merge_results.1?;

    // Verify both merges succeeded
    assert!(dest_dir1.join("merged/bin").exists());
    assert!(dest_dir2.join("merged/bin").exists());

    // Cleanup
    let cleanup_results = tokio::join!(merger1.unmount(), merger2.unmount());
    cleanup_results.0?;
    cleanup_results.1?;

    Ok(())
}

#[test_log::test(tokio::test)]
#[ignore = "requires pulling Docker images"]
async fn test_oci_merge_error_handling() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;

    // Try to merge non-existent image
    let dest_dir = temp_dir.path().join("merged_error_test");
    fs::create_dir_all(&dest_dir)?;

    let merger = OverlayFsMerger::new(temp_dir.path(), dest_dir.clone());

    // This should fail because no image was pulled
    let result = merger.merge("nonexistent_image").await;
    assert!(result.is_err());

    // Verify cleanup happened despite error
    assert!(!dest_dir.join("work").exists());
    assert!(!dest_dir.join("upper").exists());
    assert!(!dest_dir.join("merged").exists());

    Ok(())
}