use git2::{BranchType, Repository};
use std::path::PathBuf;
use tempfile::TempDir;

use crate::SquishError;

/// Clone the test-squish repository into a temporary directory and return the path.
///
/// # Returns
/// A tuple containing the path to the cloned repository and the TempDir handle.
/// The TempDir must be kept alive to prevent the directory from being deleted.
///
/// # Example
/// ```
/// use git_squish::test_utils::clone_test_repo;
///
/// let (repo_path, _temp_dir) = clone_test_repo().unwrap();
/// // Use repo_path for testing...
/// // _temp_dir will be automatically cleaned up when dropped
/// ```
pub fn clone_test_repo() -> Result<(PathBuf, TempDir), SquishError> {
    let temp_dir = tempfile::tempdir().map_err(|e| SquishError::Other {
        message: format!("Failed to create temporary directory: {}", e),
    })?;

    let repo_path = temp_dir.path().to_path_buf();
    let test_repo_url = "https://github.com/ncipollo/test-squish";

    let repo = Repository::clone(test_repo_url, &repo_path).map_err(|e| SquishError::Other {
        message: format!(
            "Failed to clone test repository from {}: {}",
            test_repo_url, e
        ),
    })?;

    // Configure Git user for testing to avoid CI failures
    let mut config = repo.config().map_err(|e| SquishError::Other {
        message: format!("Failed to get repository config: {}", e),
    })?;

    config
        .set_str("user.name", "Test User")
        .map_err(|e| SquishError::Other {
            message: format!("Failed to set user.name: {}", e),
        })?;

    config
        .set_str("user.email", "test@example.com")
        .map_err(|e| SquishError::Other {
            message: format!("Failed to set user.email: {}", e),
        })?;

    Ok((repo_path, temp_dir))
}

/// Change to a specific branch in the given repository.
///
/// # Arguments
/// * `repo_path` - Path to the git repository
/// * `branch_name` - Name of the branch to switch to (e.g., "main", "feature-branch")
///
/// # Returns
/// Success message on completion, or a SquishError if the operation fails.
///
/// # Example
/// ```
/// use git_squish::test_utils::{clone_test_repo, change_to_branch};
///
/// let (repo_path, _temp_dir) = clone_test_repo().unwrap();
/// change_to_branch(&repo_path, "main").unwrap();
/// ```
pub fn change_to_branch(repo_path: &PathBuf, branch_name: &str) -> Result<String, SquishError> {
    let repo = Repository::open(repo_path)?;

    // First, try to find a local branch with the given name
    let branch_ref = match repo.find_branch(branch_name, BranchType::Local) {
        Ok(branch) => branch.get().name().unwrap_or_default().to_string(),
        Err(_) => {
            // If local branch doesn't exist, try to find a remote branch and create a local tracking branch
            let remote_branch_name = format!("origin/{}", branch_name);
            let remote_branch = repo
                .find_branch(&remote_branch_name, BranchType::Remote)
                .map_err(|_| SquishError::Other {
                    message: format!(
                        "Branch '{}' not found locally or as '{}'",
                        branch_name, remote_branch_name
                    ),
                })?;

            // Get the commit that the remote branch points to
            let remote_commit = remote_branch.get().peel_to_commit()?;

            // Create a local branch that tracks the remote branch
            let local_branch = repo.branch(branch_name, &remote_commit, false)?;

            // Set up tracking
            let mut local_branch_ref = local_branch.get().name().unwrap_or_default().to_string();
            if local_branch_ref.is_empty() {
                local_branch_ref = format!("refs/heads/{}", branch_name);
            }

            local_branch_ref
        }
    };

    // Set HEAD to point to the branch
    repo.set_head(&branch_ref)?;

    // Update the working directory to match the branch
    repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;

    Ok(format!(
        "✅ Successfully switched to branch '{}'",
        branch_name
    ))
}

/// Get the log message associated with the current commit in the repository.
///
/// # Arguments
/// * `repo_path` - Path to the git repository
///
/// # Returns
/// The commit message of the current HEAD commit, or a SquishError if the operation fails.
///
/// # Example
/// ```
/// use git_squish::test_utils::{clone_test_repo, get_current_commit_message};
///
/// let (repo_path, _temp_dir) = clone_test_repo().unwrap();
/// let message = get_current_commit_message(&repo_path).unwrap();
/// println!("Current commit message: {}", message);
/// ```
pub fn get_current_commit_message(repo_path: &PathBuf) -> Result<String, SquishError> {
    let repo = Repository::open(repo_path)?;

    // Get the current HEAD commit
    let head = repo.head()?;
    let commit = head.peel_to_commit()?;

    // Get the commit message
    let message = commit.message().ok_or_else(|| SquishError::Other {
        message: "Current commit has no message".to_string(),
    })?;

    Ok(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_test_repo() {
        let result = clone_test_repo();
        assert!(
            result.is_ok(),
            "Failed to clone test repository: {:?}",
            result.err()
        );

        let (repo_path, _temp_dir) = result.unwrap();
        assert!(repo_path.exists(), "Repository path should exist");
        assert!(
            repo_path.join(".git").exists(),
            "Should be a git repository"
        );
    }

    #[test]
    fn test_change_to_branch() {
        let (repo_path, _temp_dir) = clone_test_repo().unwrap();

        // Test switching to main branch (should already be on main)
        let result = change_to_branch(&repo_path, "main");
        assert!(
            result.is_ok(),
            "Failed to switch to main branch: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_get_current_commit_message() {
        let (repo_path, _temp_dir) = clone_test_repo().unwrap();

        let result = get_current_commit_message(&repo_path);
        assert!(
            result.is_ok(),
            "Failed to get current commit message: {:?}",
            result.err()
        );

        let message = result.unwrap();
        assert!(!message.is_empty(), "Commit message should not be empty");
    }

    #[test]
    fn test_full_workflow() {
        // Test the complete workflow: clone -> change branch -> get message
        let (repo_path, _temp_dir) = clone_test_repo().unwrap();

        change_to_branch(&repo_path, "main").unwrap();
        let message = get_current_commit_message(&repo_path).unwrap();

        assert!(!message.is_empty(), "Should have a commit message");
    }
}
