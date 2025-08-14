use git2::{
    Commit, Error, RebaseOptions, Repository,
};

/// Rebase `branch_refname` onto `upstream_spec` (e.g. "main" or "origin/main"),
/// then replace the branch history with a **single squashed commit**.
fn main() -> Result<(), Error> {
    // args: [branch-refname] <upstream-spec>
    // ex:   refs/heads/feature  origin/main
    // ex:   origin/main         (uses current branch)
    let args = std::env::args().skip(1);
    let repo_path = ".";
    
    let repo = Repository::open(repo_path)?;
    
    // Determine branch and upstream from remaining args
    let remaining_args: Vec<String> = args.collect();
    let (branch_refname, upstream_spec) = match remaining_args.len() {
        1 => {
            // Only upstream specified, use current branch
            let current_branch = get_current_branch_name(&repo)?;
            (current_branch, remaining_args[0].clone())
        }
        2 => {
            // Both branch and upstream specified
            (remaining_args[0].clone(), remaining_args[1].clone())
        }
        _ => {
            eprintln!("Usage: git-squish [branch-refname] <upstream-spec>");
            eprintln!("  If branch-refname is omitted, uses the current branch");
            eprintln!("Examples:");
            eprintln!("  git-squish refs/heads/feature origin/main");
            eprintln!("  git-squish origin/main  # uses current branch");
            std::process::exit(1);
        }
    };

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
        None,               // Don't update any reference yet
        &sig,               // author
        &sig,               // committer
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

    println!("Squashed {} onto {} as {}", branch_refname, upstream_spec, new_commit_id);
    Ok(())
}

/// Get the current branch name from the repository's HEAD.
/// Returns the full reference name (e.g., "refs/heads/feature").
fn get_current_branch_name(repo: &Repository) -> Result<String, Error> {
    let head = repo.head()?;
    
    if let Some(name) = head.name() {
        Ok(name.to_string())
    } else {
        // HEAD is detached, get the current commit and find which branch points to it
        let head_commit = head.target().ok_or_else(|| {
            Error::from_str("HEAD does not point to a valid commit")
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
        
        Err(Error::from_str("Cannot determine current branch - HEAD is detached and no branch points to current commit"))
    }
}

/// Build a human-friendly squash message from the rebased range.
/// This scans commits reachable from `rebased_tip` back to (but excluding) `upstream_parent`.
fn build_squash_message(repo: &Repository, upstream_parent: &Commit, rebased_tip: &Commit) -> Result<String, Error> {
    // Walk from rebased_tip back until we hit upstream_parent.
    let mut revwalk = repo.revwalk()?;
    revwalk.push(rebased_tip.id())?;
    revwalk.hide(upstream_parent.id())?;
    revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)?;

    let mut subjects: Vec<String> = Vec::new();
    for oid in revwalk {
        let oid = oid?;
        let c = repo.find_commit(oid)?;
        let s = c.summary().unwrap_or("(no subject)").to_owned();
        subjects.push(format!("* {}", s));
    }

    let title = if let Some(first) = subjects.first().cloned() {
        // Strip the leading bullet for the title line.
        first.trim_start_matches("* ").to_string()
    } else {
        "Squashed commit".to_string()
    };

    let mut msg = String::new();
    msg.push_str(&title);
    msg.push_str("\n\nSquashed commits:\n");
    for s in subjects {
        msg.push_str(&s);
        msg.push('\n');
    }
    Ok(msg)
}
