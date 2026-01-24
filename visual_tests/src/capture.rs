use crate::{Result, VisualTestError};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

/// Configuration for capturing a screenshot
pub struct CaptureConfig {
    /// Name of the example to run
    pub example_name: String,
    /// Path where the screenshot will be saved
    pub output_path: PathBuf,
    /// Delay in milliseconds to wait for rendering to stabilize
    pub stabilization_delay_ms: u64,
}

/// Capture a screenshot of a running example
pub fn capture_example(config: &CaptureConfig) -> Result<()> {
    let workspace_dir = env!("CARGO_MANIFEST_DIR").replace("/visual_tests", "");

    // Build the example first
    let build_output = Command::new("cargo")
        .args(["build", "--example", &config.example_name])
        .current_dir(&workspace_dir)
        .output()
        .map_err(|e| VisualTestError::Capture(format!("Failed to run cargo build: {}", e)))?;

    if !build_output.status.success() {
        let stderr = String::from_utf8_lossy(&build_output.stderr);
        let stdout = String::from_utf8_lossy(&build_output.stdout);
        return Err(VisualTestError::Capture(format!(
            "Example '{}' failed to build:\nstdout: {}\nstderr: {}",
            config.example_name, stdout, stderr
        )));
    }

    // Start the example process
    let mut example_process = Command::new("cargo")
        .args(["run", "--example", &config.example_name])
        .current_dir(&workspace_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| VisualTestError::Capture(format!("Failed to start example: {}", e)))?;

    // Wait for the example to render
    thread::sleep(Duration::from_millis(config.stabilization_delay_ms));

    // Capture screenshot with grim
    let grim_output = Command::new("grim")
        .args(["-o", "HEADLESS-1", config.output_path.to_str().unwrap()])
        .output()
        .map_err(|e| VisualTestError::Capture(format!("Failed to run grim: {}", e)))?;

    // Kill the example process
    let _ = example_process.kill();
    let _ = example_process.wait();

    if !grim_output.status.success() {
        let stderr = String::from_utf8_lossy(&grim_output.stderr);
        return Err(VisualTestError::Capture(format!(
            "grim failed to capture screenshot: {}",
            stderr
        )));
    }

    // Verify the screenshot was created
    if !config.output_path.exists() {
        return Err(VisualTestError::Capture(format!(
            "Screenshot was not created at {}",
            config.output_path.display()
        )));
    }

    Ok(())
}
