use crate::git;
use crate::repo::{Repo, Worktree};
use anyhow::{Result, bail};

/// Remove each worktree with `git worktree remove`. Then delete its branch.
///
/// Git refuses to remove a worktree that has a modified tracked file. Git
/// also refuses if an untracked file is present. Git does not count the
/// files that it ignores. An ignored file that is not in the store is
/// therefore lost, and the user sees no warning.
///
/// The command reads every name first. A name that is not a worktree stops
/// the command before it removes anything, because a list with a typo in it
/// is a list the user wrote by mistake.
pub fn run(repo: &Repo, targets: &[String], force: bool) -> Result<()> {
    let worktrees = repo.worktrees()?;
    let chosen = resolve(&worktrees, targets)?;

    let mut kept = Vec::new();
    for wt in &chosen {
        let path = wt.path.to_string_lossy().into_owned();
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(&path);
        // Git prints the reason. Do not print it a second time.
        if git::passthrough(&repo.common, &args).is_err() {
            kept.push(wt.name());
            continue;
        }
        eprintln!("  removed    {}", wt.name());

        if let Some(branch) = &wt.branch {
            match git::out(&repo.common, &["branch", "-d", branch]) {
                Ok(_) => eprintln!("  deleted    branch {branch}"),
                Err(_) => eprintln!("  kept       branch {branch} (git did not merge it)"),
            }
        }
    }

    // One name gives one line, and that line is the whole report. A list
    // needs the count, because git wrote between the lines of wt.
    if chosen.len() > 1 {
        eprintln!("{} worktree(s) removed", chosen.len() - kept.len());
    }
    if !kept.is_empty() {
        bail!("wt cannot remove {}. use --force", kept.join(", "));
    }
    Ok(())
}

/// Find the worktree of each name, in the order the user gave.
///
/// A name that the user gives twice names one worktree, and the second
/// removal of it would fail with a reason that says nothing. Keep the first
/// of the two.
fn resolve<'a>(worktrees: &'a [Worktree], targets: &[String]) -> Result<Vec<&'a Worktree>> {
    let mut chosen: Vec<&Worktree> = Vec::with_capacity(targets.len());
    for target in targets {
        let found = worktrees
            .iter()
            .find(|w| w.name() == *target || w.path.to_string_lossy() == *target)
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
        if !chosen.iter().any(|w| w.path == found.path) {
            chosen.push(found);
        }
    }
    Ok(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn worktrees() -> Vec<Worktree> {
        ["/repo/main", "/repo/eng-1", "/repo/eng-2"]
            .iter()
            .map(|p| Worktree {
                path: PathBuf::from(p),
                branch: None,
            })
            .collect()
    }

    fn names(chosen: &[&Worktree]) -> Vec<String> {
        chosen.iter().map(|w| w.name()).collect()
    }

    #[test]
    fn the_tool_keeps_the_order_the_user_gave() {
        let all = worktrees();
        let targets = ["eng-2".to_string(), "main".to_string()];
        let chosen = resolve(&all, &targets).unwrap();
        assert_eq!(names(&chosen), ["eng-2", "main"]);
    }

    #[test]
    fn a_path_names_a_worktree_too() {
        let all = worktrees();
        let targets = ["/repo/eng-1".to_string()];
        let chosen = resolve(&all, &targets).unwrap();
        assert_eq!(names(&chosen), ["eng-1"]);
    }

    #[test]
    fn one_worktree_that_the_user_names_twice_is_one_worktree() {
        let all = worktrees();
        let targets = [
            "eng-1".to_string(),
            "/repo/eng-1".to_string(),
            "eng-2".to_string(),
        ];
        let chosen = resolve(&all, &targets).unwrap();
        assert_eq!(names(&chosen), ["eng-1", "eng-2"]);
    }

    #[test]
    fn a_name_that_is_not_a_worktree_stops_the_command() {
        let all = worktrees();
        let targets = ["eng-1".to_string(), "eng-9".to_string()];
        assert!(resolve(&all, &targets).is_err());
    }
}
