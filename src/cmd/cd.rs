use crate::repo::Repo;
use anyhow::{Result, bail};

/// Print the path of one worktree, so the shell can change directory.
///
/// The name of a worktree directory and the name of its branch can differ,
/// because a branch can move after you make the worktree. This command
/// therefore accepts either name, and it also accepts the start of a
/// directory name when only one worktree matches.
pub fn run(repo: &Repo, target: &str) -> Result<()> {
    let worktrees = repo.worktrees()?;

    if let Some(w) = worktrees.iter().find(|w| w.name() == target) {
        println!("{}", w.path.display());
        return Ok(());
    }

    if let Some(w) = worktrees
        .iter()
        .find(|w| w.branch.as_deref() == Some(target))
    {
        eprintln!("  {}  (branch {target})", w.name());
        println!("{}", w.path.display());
        return Ok(());
    }

    let matches: Vec<_> = worktrees
        .iter()
        .filter(|w| w.name().starts_with(target))
        .collect();
    match matches.as_slice() {
        [w] => {
            eprintln!("  {}", w.name());
            println!("{}", w.path.display());
            Ok(())
        }
        [] => bail!(
            "there is no worktree with the name {target}\n  the names are: {}",
            names(&worktrees)
        ),
        many => bail!(
            "{target} matches more than one worktree: {}",
            many.iter().map(|w| w.name()).collect::<Vec<_>>().join(", ")
        ),
    }
}

fn names(worktrees: &[crate::repo::Worktree]) -> String {
    worktrees
        .iter()
        .map(|w| w.name())
        .collect::<Vec<_>>()
        .join(", ")
}
