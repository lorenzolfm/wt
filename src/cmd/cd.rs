use crate::cmd::follow;
use crate::repo::{Repo, Worktree};
use anyhow::{Result, bail};

/// Print the path of one worktree, so the shell can change directory.
///
/// The name of a worktree directory and the name of its branch can differ,
/// because a branch can move after you make the worktree. This command
/// therefore accepts either name, and it also accepts the start of a
/// directory name when only one worktree matches.
///
/// With `fetch`, the command also fetches the upstream branch of the
/// worktree and tells what is new there. It changes no branch. A failed
/// fetch is a warning, and the command still prints the path.
pub fn run(repo: &Repo, target: &str, fetch: bool) -> Result<()> {
    go(&lookup(repo, target, "worktree")?, fetch);
    Ok(())
}

/// Handle `wt <worktree>`, the short form of `wt cd <worktree>`.
///
/// Clap sends every unknown first word here, so a mistyped command arrives
/// with the worktree names. A command therefore always wins over a worktree
/// with the same name, and `wt cd <name>` reaches such a worktree.
///
/// The words after the name can be `-f` or `--fetch`, as for `wt cd`. Clap
/// reads an option before the name as an option of wt itself, so the option
/// goes after the name.
pub fn bare(repo: &Repo, words: &[String]) -> Result<()> {
    let (target, fetch) = match words {
        [target] => (target, false),
        [target, option] if is_fetch(option) => (target, true),
        [first, ..] => bail!(
            "there is no command with the name {first}\n  wt <worktree> is the short form of wt cd <worktree>, and it takes one name and -f"
        ),
        [] => unreachable!("clap gives at least one word"),
    };
    go(&lookup(repo, target, "command or worktree")?, fetch);
    Ok(())
}

fn is_fetch(word: &str) -> bool {
    word == "-f" || word == "--fetch"
}

/// Print the path of `w`, after the report of its upstream branch when
/// `fetch` is set.
fn go(w: &Worktree, fetch: bool) {
    if fetch && let Err(e) = follow::check(&w.path) {
        eprintln!("warning: wt cannot tell what is new upstream: {e:#}");
    }
    println!("{}", w.path.display());
}

/// Find the worktree that `target` names. `noun` names what the caller
/// looked for, for the message when nothing matches.
fn lookup(repo: &Repo, target: &str, noun: &str) -> Result<Worktree> {
    let mut worktrees = repo.worktrees()?;

    if let Some(i) = worktrees.iter().position(|w| w.name() == target) {
        return Ok(worktrees.swap_remove(i));
    }

    if let Some(i) = worktrees
        .iter()
        .position(|w| w.branch.as_deref() == Some(target))
    {
        let w = worktrees.swap_remove(i);
        eprintln!("  {}  (branch {target})", w.name());
        return Ok(w);
    }

    let matches: Vec<_> = worktrees
        .iter()
        .filter(|w| w.name().starts_with(target))
        .collect();
    match matches.as_slice() {
        [w] => {
            eprintln!("  {}", w.name());
            Ok((*w).clone())
        }
        [] => bail!(
            "there is no {noun} with the name {target}\n  the worktree names are: {}",
            names(&worktrees)
        ),
        many => bail!(
            "{target} matches more than one worktree: {}",
            many.iter().map(|w| w.name()).collect::<Vec<_>>().join(", ")
        ),
    }
}

fn names(worktrees: &[Worktree]) -> String {
    worktrees
        .iter()
        .map(|w| w.name())
        .collect::<Vec<_>>()
        .join(", ")
}
