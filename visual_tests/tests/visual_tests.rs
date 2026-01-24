use visual_tests::{run_visual_test, should_update_references, update_reference, VisualTestConfig};

/// Helper macro to generate visual test functions
macro_rules! visual_test {
    ($name:ident, $example:literal) => {
        #[test]
        fn $name() {
            if should_update_references() {
                update_reference($example, 1000).expect("Failed to update reference");
                return;
            }

            let result = run_visual_test(&VisualTestConfig {
                example_name: $example.to_string(),
                stabilization_delay_ms: 1000,
                similarity_threshold: 0.999, // 99.9% - strict to catch layout changes
            })
            .expect("Visual test failed to run");

            assert!(
                result.passed,
                "Visual regression detected for '{}': similarity {:.4}% (threshold: 99.9%)\n\
                 Reference: {}\n\
                 Captured:  {}\n\
                 Diff:      {}",
                $example,
                result.similarity * 100.0,
                result.reference_path.display(),
                result.captured_path.display(),
                result
                    .diff_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "N/A".to_string())
            );
        }
    };
}

// Core showcase tests
visual_test!(test_showcase, "showcase");
visual_test!(test_state_layer, "state_layer_example");
visual_test!(test_flex_layout, "flex_layout_test");
visual_test!(test_transform, "transform_example");
visual_test!(test_animation, "animation_example");
visual_test!(test_elevation, "elevation_example");
visual_test!(test_image, "image_example");
visual_test!(test_reactive, "reactive_example");
visual_test!(test_component, "component_example");
visual_test!(test_children, "children_example");
