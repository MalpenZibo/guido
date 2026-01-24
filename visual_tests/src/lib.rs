mod capture;
mod compare;

pub use capture::{capture_example, CaptureConfig};
pub use compare::{compare_images, generate_diff_image, CompareResult};

use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum VisualTestError {
    #[error("Failed to capture screenshot: {0}")]
    Capture(String),
    #[error("Failed to compare images: {0}")]
    Compare(String),
    #[error("Reference image not found: {0}")]
    ReferenceNotFound(PathBuf),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),
}

pub type Result<T> = std::result::Result<T, VisualTestError>;

/// Configuration for a visual test
#[derive(Clone)]
pub struct VisualTestConfig {
    /// Name of the example to run
    pub example_name: String,
    /// Delay in milliseconds to wait for rendering to stabilize
    pub stabilization_delay_ms: u64,
    /// Similarity threshold (0.0 to 1.0, default 0.99)
    pub similarity_threshold: f64,
}

impl Default for VisualTestConfig {
    fn default() -> Self {
        Self {
            example_name: String::new(),
            stabilization_delay_ms: 1000,
            similarity_threshold: 0.99,
        }
    }
}

/// Result of a visual test
pub struct VisualTestResult {
    /// Whether the test passed (similarity >= threshold)
    pub passed: bool,
    /// The similarity score (0.0 to 1.0)
    pub similarity: f64,
    /// Path to the captured screenshot
    pub captured_path: PathBuf,
    /// Path to the reference image
    pub reference_path: PathBuf,
    /// Path to diff image (if generated on failure)
    pub diff_path: Option<PathBuf>,
}

/// Get the path to the references directory
pub fn references_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("references")
}

/// Get the path to a reference image for an example
pub fn reference_path(example_name: &str) -> PathBuf {
    references_dir().join(format!("{}.png", example_name))
}

/// Get the path to the output directory for test artifacts
pub fn output_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("output")
}

/// Get the path to a captured screenshot
pub fn captured_path(example_name: &str) -> PathBuf {
    output_dir().join(format!("{}_captured.png", example_name))
}

/// Get the path to a diff image
pub fn diff_path(example_name: &str) -> PathBuf {
    output_dir().join(format!("{}_diff.png", example_name))
}

/// Run a visual regression test
pub fn run_visual_test(config: &VisualTestConfig) -> Result<VisualTestResult> {
    // Ensure output directory exists
    std::fs::create_dir_all(output_dir())?;

    let ref_path = reference_path(&config.example_name);
    let cap_path = captured_path(&config.example_name);

    // Check if reference exists
    if !ref_path.exists() {
        return Err(VisualTestError::ReferenceNotFound(ref_path));
    }

    // Capture screenshot
    let capture_config = CaptureConfig {
        example_name: config.example_name.clone(),
        output_path: cap_path.clone(),
        stabilization_delay_ms: config.stabilization_delay_ms,
    };
    capture_example(&capture_config)?;

    // Compare images
    let compare_result = compare_images(&ref_path, &cap_path)?;
    let passed = compare_result.similarity >= config.similarity_threshold;

    // Generate diff if failed
    let diff = if !passed {
        let diff_file = diff_path(&config.example_name);
        generate_diff_image(&ref_path, &cap_path, &diff_file)?;
        Some(diff_file)
    } else {
        None
    };

    Ok(VisualTestResult {
        passed,
        similarity: compare_result.similarity,
        captured_path: cap_path,
        reference_path: ref_path,
        diff_path: diff,
    })
}

/// Update the reference image for an example
pub fn update_reference(example_name: &str, stabilization_delay_ms: u64) -> Result<PathBuf> {
    // Ensure references directory exists
    std::fs::create_dir_all(references_dir())?;

    let ref_path = reference_path(example_name);

    // Capture screenshot directly to reference path
    let capture_config = CaptureConfig {
        example_name: example_name.to_string(),
        output_path: ref_path.clone(),
        stabilization_delay_ms,
    };
    capture_example(&capture_config)?;

    println!("Updated reference: {}", ref_path.display());
    Ok(ref_path)
}

/// Check if we're in update references mode
pub fn should_update_references() -> bool {
    std::env::var("UPDATE_REFERENCES").is_ok()
}
