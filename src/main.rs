use git_squish::SquishError;
use git2::Repository;

fn main() {
    if let Err(e) = run() {
        eprintln!("💀 Error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), SquishError> {
    // args: [branch-refname] <upstream-spec>
    // ex:   refs/heads/feature  origin/main
    // ex:   origin/main         (uses current branch)
    let args = std::env::args().skip(1);
    let repo_path = ".";

    // Determine branch and upstream from remaining args
    let remaining_args: Vec<String> = args.collect();
    let (branch_refname, upstream_spec) = match remaining_args.len() {
        1 => {
            // Only upstream specified, use current branch
            let repo = Repository::open(repo_path)?;
            let current_branch = git_squish::get_current_branch_name(&repo)?;
            (current_branch, remaining_args[0].clone())
        }
        2 => {
            // Both branch and upstream specified
            (remaining_args[0].clone(), remaining_args[1].clone())
        }
        _ => {
            eprintln!("Usage: git squish [branch-refname] <upstream-spec>");
            eprintln!("  If branch-refname is omitted, uses the current branch");
            eprintln!("Examples:");
            eprintln!("  git squish topic main");
            eprintln!("  git squish main  # uses current branch");
            std::process::exit(1);
        }
    };

    // Perform the squash operation
    let result = git_squish::squash_branch(repo_path, branch_refname, upstream_spec)?;
    println!("{result}");
    Ok(())
}
