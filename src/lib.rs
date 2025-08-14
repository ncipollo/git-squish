use git2::{Commit, RebaseOptions, Repository};

mod error;
pub use error::SquishError;

#[cfg(test)]
pub mod test_utils;

/// Squash a branch onto an upstream branch, replacing the branch history with a single commit.
///
/// # Arguments
/// * `repo_path` - Path to the git repository
/// * `branch_refname` - The branch to squash (e.g., "refs/heads/feature")
/// * `upstream_spec` - The upstream to rebase onto (e.g., "main" or "origin/main")
///
/// # Returns
/// A success message on completion, or a SquishError if the operation fails.
pub fn squash_branch(
    repo_path: &str,
    branch_refname: String,
    upstream_spec: String,
) -> Result<String, SquishError> {
    let repo = Repository::open(repo_path)?;

    // Resolve the branch head to an AnnotatedCommit.
    let branch_ref = repo.find_reference(&branch_refname)?;
    let branch_annot = repo.reference_to_annotated_commit(&branch_ref)?;

    // Resolve upstream (you may pass "main" or "origin/main" etc.).
    let upstream_obj = repo.revparse_single(&upstream_spec)?;
    let upstream_id = upstream_obj.id();
    let upstream_annot = repo.find_annotated_commit(upstream_id)?;

    // --- 1) Standard rebase to linearize the topic branch onto upstream ---
    let mut opts = RebaseOptions::new();
    // In-memory avoids touching the worktree while applying; safer for automation.
    opts.inmemory(true);

    let mut rebase = repo.rebase(
        Some(&branch_annot),
        Some(&upstream_annot),
        None,
        Some(&mut opts),
    )?;

    // Apply each operation and commit it (in-memory).
    let sig = repo.signature()?;
    while let Some(op_result) = rebase.next() {
        let _op = op_result?;
        // If there are conflicts, you'd inspect `rebase.inmemory_index()?` and resolve.
        // For brevity we assume clean application.
        rebase.commit(Some(&sig), &sig, None)?;
    }
    // Finalize the rebase (updates the branch ref to the rebased tip).
    rebase.finish(None)?;

    // Fetch the rebased branch tip and its tree.
    let rebased_tip_id = repo.refname_to_id(&branch_refname)?;
    let rebased_tip = repo.find_commit(rebased_tip_id)?;
    let rebased_tree = rebased_tip.tree()?;

    // --- 2) "Squash" by replacing the rebased linear series with ONE commit ---
    // Parent of the squash commit is the upstream commit we rebased onto.
    let upstream_parent = repo.find_commit(upstream_id)?;

    // Compose a sensible commit message:
    //   - take the first (oldest) commit's subject + append shortened list
    //     of included commits (optional, tweak as you like).
    let message = build_squash_message(&repo, &upstream_parent, &rebased_tip)?;

    // Create a *new* commit that has:
    //   - the exact tree of the rebased tip (i.e., all changes combined)
    //   - a single parent: the upstream base
    //   - but don't update the branch ref yet (do it manually afterward)
    let new_commit_id = repo.commit(
        None, // Don't update any reference yet
        &sig, // author
        &sig, // committer
        &message,
        &rebased_tree,
        &[&upstream_parent],
    )?;

    // Now manually update the branch reference to point to our new squashed commit
    let mut branch_ref = repo.find_reference(&branch_refname)?;
    branch_ref.set_target(new_commit_id, "squash commits into single commit")?;

    // Optional: force-move HEAD if it was on this branch (useful in detached states etc.).
    if let Ok(mut head) = repo.head() {
        if head.is_branch() && head.name() == Some(branch_refname.as_str()) {
            head.set_target(new_commit_id, "move HEAD to squashed commit")?;
        }
    }

    Ok(format!(
        "✅ Successfully rebased and updated {branch_refname}."
    ))
}

/// Get the current branch name from the repository's HEAD.
/// Returns the full reference name (e.g., "refs/heads/feature").
pub fn get_current_branch_name(repo: &Repository) -> Result<String, SquishError> {
    let head = repo.head()?;

    if let Some(name) = head.name() {
        Ok(name.to_string())
    } else {
        // HEAD is detached, get the current commit and find which branch points to it
        let head_commit = head.target().ok_or_else(|| SquishError::Other {
            message: "HEAD does not point to a valid commit".to_string(),
        })?;

        // Look for a branch that points to the same commit
        let mut branches = repo.branches(Some(git2::BranchType::Local))?;
        for branch_result in &mut branches {
            let (branch, _) = branch_result?;
            if let Some(target) = branch.get().target() {
                if target == head_commit {
                    if let Some(branch_name) = branch.get().name() {
                        return Ok(branch_name.to_string());
                    }
                }
            }
        }

        Err(SquishError::Other {
            message: "Cannot determine current branch - HEAD is detached and no branch points to current commit".to_string(),
        })
    }
}

/// Build a squash message using the message from the first commit.
/// This scans commits reachable from `rebased_tip` back to (but excluding) `upstream_parent`
/// and returns the full message from the first (oldest) commit.
fn build_squash_message(
    repo: &Repository,
    upstream_parent: &Commit,
    rebased_tip: &Commit,
) -> Result<String, SquishError> {
    // Walk from rebased_tip back until we hit upstream_parent.
    let mut revwalk = repo.revwalk()?;
    revwalk.push(rebased_tip.id())?;
    revwalk.hide(upstream_parent.id())?;
    revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)?;

    // Get the first commit in the range
    if let Some(first_oid) = revwalk.next() {
        let first_oid = first_oid?;
        let first_commit = repo.find_commit(first_oid)?;
        // Return the full message from the first commit
        first_commit
            .message()
            .ok_or_else(|| SquishError::Other {
                message: "First commit has no message".to_string(),
            })
            .map(|msg| msg.to_string())
    } else {
        Err(SquishError::Other {
            message: "No commits found in the range to squash".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{change_to_branch, clone_test_repo, get_current_commit_message};
    use std::fs;

    /// Read the contents of a file in the repository.
    fn read_file_contents(
        repo_path: &std::path::PathBuf,
        filename: &str,
    ) -> Result<String, SquishError> {
        let file_path = repo_path.join(filename);
        fs::read_to_string(file_path).map_err(|e| SquishError::Other {
            message: format!("Failed to read file {}: {}", filename, e),
        })
    }

    #[test]
    fn test_squish_topic_branch_workflow() {
        // Clone the test repository
        let (repo_path, _temp_dir) = clone_test_repo().expect("Failed to clone test repository");

        // Checkout the topic branch
        change_to_branch(&repo_path, "topic").expect("Failed to checkout topic branch");

        // Get the current branch name (should be refs/heads/topic)
        let repo = Repository::open(&repo_path).expect("Failed to open repository");
        let branch_refname =
            get_current_branch_name(&repo).expect("Failed to get current branch name");

        // Squish the topic branch against main
        let repo_path_str = repo_path.to_str().expect("Invalid repo path");
        let result = squash_branch(repo_path_str, branch_refname, "main".to_string());

        assert!(
            result.is_ok(),
            "Squash operation failed: {:?}",
            result.err()
        );

        // Verify the log message is "Topic Branch Start"
        let commit_message =
            get_current_commit_message(&repo_path).expect("Failed to get commit message");

        assert_eq!(
            commit_message.trim(),
            "Topic Branch Start",
            "Expected commit message 'Topic Branch Start', got: '{}'",
            commit_message
        );

        // Verify the contents of text.txt
        let file_contents =
            read_file_contents(&repo_path, "text.txt").expect("Failed to read text.txt");

        let expected_contents = "\
Thu Aug 14 15:10:43 EDT 2025
Thu Aug 14 15:11:01 EDT 2025
Thu Aug 14 15:11:04 EDT 2025
Thu Aug 14 15:11:07 EDT 2025
Thu Aug 14 15:49:25 EDT 2025
";

        assert_eq!(
            file_contents, expected_contents,
            "text.txt contents don't match expected values.\nExpected:\n{}\nActual:\n{}",
            expected_contents, file_contents
        );
    }

    #[test]
    fn test_squish_conflict_branch_should_fail() {
        // Clone the test repository
        let (repo_path, _temp_dir) = clone_test_repo().expect("Failed to clone test repository");

        // Checkout the conflict branch
        change_to_branch(&repo_path, "conflict").expect("Failed to checkout conflict branch");

        // Get the current branch name (should be refs/heads/conflict)
        let repo = Repository::open(&repo_path).expect("Failed to open repository");
        let branch_refname =
            get_current_branch_name(&repo).expect("Failed to get current branch name");

        // First, make sure we have the topic branch locally
        change_to_branch(&repo_path, "topic").expect("Failed to ensure topic branch exists");
        change_to_branch(&repo_path, "conflict").expect("Failed to return to conflict branch");

        // Try to squish the conflict branch against topic - this should fail with a merge conflict
        let repo_path_str = repo_path.to_str().expect("Invalid repo path");
        let result = squash_branch(repo_path_str, branch_refname, "topic".to_string());

        // Assert that the operation failed
        assert!(
            result.is_err(),
            "Expected squash operation to fail due to merge conflict, but it succeeded"
        );

        // Verify that it's a conflict-related error
        let error = result.unwrap_err();
        match error {
            SquishError::Git { message } => {
                assert!(
                    message.contains("conflict"),
                    "Expected conflict-related error message, got: '{}'",
                    message
                );
            }
            _ => panic!(
                "Expected SquishError::Git with conflict message, got: {:?}",
                error
            ),
        }
    }
}
