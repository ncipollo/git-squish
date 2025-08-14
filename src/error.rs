use git2::{ErrorClass, ErrorCode};
use std::fmt;

/// Custom error type for git-squish operations
#[derive(Debug)]
pub enum SquishError {
    /// Git operation error with optional enhanced context
    Git { message: String },
    /// Other errors
    Other { message: String },
}

impl fmt::Display for SquishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SquishError::Git { message } => write!(f, "{}", message),
            SquishError::Other { message } => write!(f, "{}", message),
        }
    }
}

impl std::error::Error for SquishError {}

impl From<git2::Error> for SquishError {
    fn from(error: git2::Error) -> Self {
        // Check if this is a conflict-related error
        let is_conflict = match (error.class(), error.code()) {
            (ErrorClass::Merge, ErrorCode::Conflict) => true,
            (ErrorClass::Merge, ErrorCode::MergeConflict) => true,
            (ErrorClass::Index, ErrorCode::Unmerged) => true,
            (ErrorClass::Checkout, ErrorCode::Conflict) => true,
            _ => {
                // Also check the error message for conflict-related keywords
                let msg = error.message().to_lowercase();
                msg.contains("conflict") || (msg.contains("merge") && msg.contains("failed"))
            }
        };

        let message = if is_conflict {
            "There was a conflict during this squish, please retry using git rebase -i and resolve the conflicts".to_string()
        } else {
            error.message().to_string()
        };

        SquishError::Git { message }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Error;
    use std::error::Error as StdError;

    #[test]
    fn test_squish_error_display() {
        let git_error = SquishError::Git {
            message: "Test git error".to_string(),
        };
        assert_eq!(format!("{}", git_error), "Test git error");

        let other_error = SquishError::Other {
            message: "Test other error".to_string(),
        };
        assert_eq!(format!("{}", other_error), "Test other error");
    }

    #[test]
    fn test_squish_error_debug() {
        let error = SquishError::Git {
            message: "Debug test".to_string(),
        };
        let debug_output = format!("{:?}", error);
        assert!(debug_output.contains("Git"));
        assert!(debug_output.contains("Debug test"));
    }

    #[test]
    fn test_from_git2_conflict_error() {
        // Create a mock conflict error
        let git_error = Error::from_str("merge conflict in file.txt");
        let squish_error = SquishError::from(git_error);

        if let SquishError::Git { message } = squish_error {
            assert_eq!(
                message,
                "There was a conflict during this squish, please retry using git rebase -i and resolve the conflicts"
            );
        } else {
            panic!("Expected Git error variant");
        }
    }

    #[test]
    fn test_from_git2_non_conflict_error() {
        let git_error = Error::from_str("repository not found");
        let squish_error = SquishError::from(git_error);

        if let SquishError::Git { message } = squish_error {
            assert_eq!(message, "repository not found");
        } else {
            panic!("Expected Git error variant");
        }
    }

    #[test]
    fn test_from_git2_merge_failed_error() {
        let git_error = Error::from_str("merge failed due to conflicts");
        let squish_error = SquishError::from(git_error);

        if let SquishError::Git { message } = squish_error {
            assert_eq!(
                message,
                "There was a conflict during this squish, please retry using git rebase -i and resolve the conflicts"
            );
        } else {
            panic!("Expected Git error variant");
        }
    }

    #[test]
    fn test_conflict_detection_keywords() {
        let test_cases = vec![
            ("conflict in file", true),
            ("CONFLICT in file", true),
            ("merge failed", true),
            ("repository not found", false),
            ("invalid reference", false),
            ("nothing to merge failed", true), // contains "merge" and "failed"
        ];

        for (error_msg, should_be_conflict) in test_cases {
            let git_error = Error::from_str(error_msg);
            let squish_error = SquishError::from(git_error);

            if let SquishError::Git { message } = squish_error {
                if should_be_conflict {
                    assert_eq!(
                        message,
                        "There was a conflict during this squish, please retry using git rebase -i and resolve the conflicts",
                        "Error message '{}' should be detected as conflict",
                        error_msg
                    );
                } else {
                    assert_eq!(
                        message, error_msg,
                        "Error message '{}' should not be detected as conflict",
                        error_msg
                    );
                }
            } else {
                panic!("Expected Git error variant");
            }
        }
    }

    #[test]
    fn test_error_trait_implementation() {
        let error = SquishError::Other {
            message: "test error".to_string(),
        };

        // Test that it implements std::error::Error
        let _error_ref: &dyn std::error::Error = &error;

        // Test source method (should return None for our simple implementation)
        assert!(StdError::source(&error).is_none());
    }
}
