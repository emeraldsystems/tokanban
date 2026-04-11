/// Common test utilities, fixtures, and helpers

pub mod mock_server;
pub mod fixtures;

pub use mock_server::MockServer;
pub use fixtures::*;

/// Create a temporary config directory for tests
pub fn setup_temp_config() -> tempfile::TempDir {
    tempfile::tempdir().expect("failed to create temp dir")
}

/// Helper to create a test workspace slug
pub fn test_workspace() -> String {
    "test-workspace".to_string()
}

/// Helper to create a test project key
pub fn test_project() -> String {
    "TEST".to_string()
}

/// Helper to create a test user ID
pub fn test_user_id() -> String {
    "user-abc123".to_string()
}

/// Helper for test assertions on error types
pub mod assertions {
    use assert_matches::assert_matches;

    /// Assert that a result contains a specific error message
    pub fn assert_error_message<T, E: std::fmt::Display>(
        result: Result<T, E>,
        expected_msg: &str,
    ) {
        match result {
            Ok(_) => panic!("expected error, got Ok"),
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains(expected_msg),
                    "expected error containing '{}', got '{}'",
                    expected_msg,
                    msg
                );
            }
        }
    }
}
