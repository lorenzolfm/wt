use crate::git;
use crate::repo::Repo;
use anyhow::{Result, bail};

/// Remove a worktree with `git worktree remove`. Then delete its branch.
///
/// Git refuses to remove a worktree that has a modified tracked file. Git
/// also refuses if an untracked file is present. Git does not count the
/// files that it ignores. An ignored file that is not in the store is
/// therefore lost, and the user sees no warning.
pub fn run(repo: &Repo, target: &str, force: bool) -> Result<()> {
    let worktrees = repo.worktrees()?;
    let found = worktrees
        .iter()
        .find(|w| w.name() == target || w.path.to_string_lossy() == target)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "there is no worktree with the name {target}\n  the names are: {}",
                worktrees
                    .iter()
                    .map(|w| w.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;

    let path = found.path.to_string_lossy().into_owned();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&path);
    // Git prints the reason. Do not print it a second time.
    if git::passthrough(&repo.common, &args).is_err() {
        bail!(
            "wt cannot remove {}. use --force to remove it",
            found.name()
        );
    }
    eprintln!("  removed    {}", found.name());

    if let Some(branch) = &found.branch {
        match git::out(&repo.common, &["branch", "-d", branch]) {
            Ok(_) => eprintln!("  deleted    branch {branch}"),
            Err(_) => eprintln!("  kept       branch {branch} (git did not merge it)"),
        }
    }
    Ok(())
}
