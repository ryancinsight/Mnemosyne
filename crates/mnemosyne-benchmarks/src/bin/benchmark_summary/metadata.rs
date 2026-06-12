use serde_json::json;
use std::fs::{self, File};
use std::io;
use std::path::Path;

pub fn write_metadata_json(path: &str) -> io::Result<()> {
    let rustc_version = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let os_family = if cfg!(target_family = "windows") {
        "windows"
    } else if cfg!(target_family = "unix") {
        "unix"
    } else {
        "unknown"
    };

    let target_arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "unknown"
    };

    let timestamp_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let metadata = json!({
        "rustc_version": rustc_version,
        "os_family": os_family,
        "target_arch": target_arch,
        "timestamp_secs": timestamp_secs,
    });

    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    serde_json::to_writer_pretty(&mut file, &metadata).map_err(io::Error::other)?;
    Ok(())
}
