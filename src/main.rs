use git2::{
    Commit, Error, RebaseOptions, Repository,
};

/// Rebase `branch_refname` onto `upstream_spec` (e.g. "main" or "origin/main"),
/// then replace the branch history with a **single squashed commit**.
fn main() -> Result<(), Error> {
    // args: <repo-path> <branch-refname> <upstream-spec>
    // ex:   .            refs/heads/feature  origin/main
    let mut args = std::env::args().skip(1);
    let repo_path = args.next().unwrap_or_else(|| ".".into());
    let branch_refname = args.next().expect("branch refname, e.g. refs/heads/feature");
    let upstream_spec = args.next().expect("upstream spec, e.g. origin/main");

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
    //   - and update the branch ref to point to this new commit.
    let new_commit_id = repo.commit(
        Some(&branch_refname),
        &sig,               // author
        &sig,               // committer
        &message,
        &rebased_tree,
        &[&upstream_parent],
    )?;

    // Optional: force-move HEAD if it was on this branch (useful in detached states etc.).
    if let Ok(mut head) = repo.head() {
        if head.is_branch() && head.name() == Some(branch_refname.as_str()) {
            head.set_target(new_commit_id, "move HEAD to squashed commit")?;
        }
    }

    println!("Squashed {} onto {} as {}", branch_refname, upstream_spec, new_commit_id);
    Ok(())
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
