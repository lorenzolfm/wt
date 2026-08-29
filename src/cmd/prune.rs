use crate::git;
use crate::repo::{Repo, Worktree};
use anyhow::Result;
use std::path::Path;

/// Why a worktree is finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The base branch contains each commit of the branch.
    Merged,
    /// The branch has an upstream branch, and origin does not have it.
    Gone,
}

/// Why `wt` does not remove a worktree that is finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocker {
    /// The user is in it. A removal would leave the shell in a path that
    /// is not present.
    Current,
    /// It has a modified file or an untracked file.
    Dirty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The work is not finished. The command says nothing about it.
    Keep,
    Remove(Reason),
    Blocked(Reason, Blocker),
}

/// The facts that git gives about one worktree.
#[derive(Debug, Clone, Copy, Default)]
pub struct Facts {
    /// It is on the base branch itself, for example `main`.
    pub is_base: bool,
    /// It is on no branch.
    pub detached: bool,
    /// Origin has the branch now: `refs/remotes/origin/<branch>` is present.
    pub on_origin: bool,
    /// The base branch contains each commit of the branch.
    pub merged: bool,
    /// The config names an upstream branch, and origin does not have it.
    pub gone: bool,
    pub dirty: bool,
    pub current: bool,
}

/// Decide what to do with one worktree.
///
/// The base branch is never a candidate: each commit of the base branch is
/// in the base branch, so the test for a merge is always true for it. A
/// worktree with no branch is never a candidate either, because the command
/// has no branch to test.
///
/// Each reason needs proof that the branch reached origin. This rule is the
/// one that makes the command safe. A new branch holds no commit of its own,
/// so the base branch contains it, and the test for a merge alone cannot
/// tell `wt add eng-1234` from a branch whose work is finished.
///
/// The two reasons prove it in two ways, because a branch that origin lost
/// leaves no ref behind. A merge needs origin to have the branch now. A loss
/// needs the config to name an upstream branch that origin does not have,
/// which is what `git push -u` and `wt add <remote branch>` write.
pub fn verdict(f: &Facts, force: bool) -> Verdict {
    if f.is_base || f.detached {
        return Verdict::Keep;
    }
    // A branch can be both. The merge is the stronger fact, so it is the
    // reason that the user reads.
    let reason = if f.merged && f.on_origin {
        Reason::Merged
    } else if f.gone {
        Reason::Gone
    } else {
        return Verdict::Keep;
    };

    if f.current {
        return Verdict::Blocked(reason, Blocker::Current);
    }
    if f.dirty && !force {
        return Verdict::Blocked(reason, Blocker::Dirty);
    }
    Verdict::Remove(reason)
}

/// Remove each worktree whose work is finished.
///
/// The command first asks origin which branches are still present. It then
/// removes each worktree whose branch the base branch contains, and each
/// worktree whose upstream branch origin no longer has. The second condition
/// finds the branch that a squash merge or a rebase merge left behind: such a
/// branch is not an ancestor of the base branch, and the merge is complete
/// nonetheless.
///
/// The command removes no branch that is only on this machine. Origin must
/// have the branch, or the config must name an upstream branch that origin
/// no longer has.
///
/// The command keeps a worktree that has a modified file or an untracked
/// file. Git does not count the files that it ignores, so an ignored file
/// that is not in the store is lost without a warning, as in `wt delete`.
///
/// The main worktree of a normal repository is not removable. Git refuses,
/// and the command prints the reason git gives.
pub fn run(repo: &Repo, dry_run: bool, force: bool, fetch: bool) -> Result<()> {
    // A worktree whose directory the user deleted by hand still has its
    // administrative files. Remove them first, so that the list below holds
    // only worktrees that are present.
    stale(repo)?;

    let base = repo.default_base()?;
    // `default_base` gives a remote branch, for example `origin/main`. The
    // worktree of the base branch is on the local branch `main`.
    let base_local = base.strip_prefix("origin/").unwrap_or(&base).to_string();

    if fetch {
        eprintln!("  fetching   origin (--prune)");
        // A failure here is not fatal. Without the network the command still
        // finds each branch that the base branch contains.
        if !git::quiet(&repo.common, &["fetch", "--prune", "origin"]) {
            eprintln!("  offline    wt cannot reach origin. it uses the refs it has");
        }
    }

    let cwd = std::env::current_dir().ok().and_then(real);
    let worktrees = repo.worktrees()?;
    let mut removed = 0usize;
    let mut blocked = 0usize;

    for wt in &worktrees {
        let facts = facts(repo, wt, &base, &base_local, cwd.as_deref())?;
        match verdict(&facts, force) {
            Verdict::Keep => {}
            Verdict::Blocked(reason, blocker) => {
                blocked += 1;
                eprintln!(
                    "  kept       {}  ({}, {})",
                    wt.name(),
                    describe(reason, &base, wt),
                    match blocker {
                        Blocker::Current => "you are in it".to_string(),
                        Blocker::Dirty =>
                            "it has a modified or untracked file. use --force".to_string(),
                    }
                );
            }
            Verdict::Remove(reason) => {
                if dry_run {
                    eprintln!(
                        "  would drop {}  ({})",
                        wt.name(),
                        describe(reason, &base, wt)
                    );
                    removed += 1;
                    continue;
                }
                match remove(repo, wt, force) {
                    Ok(()) => {
                        eprintln!(
                            "  removed    {}  ({})",
                            wt.name(),
                            describe(reason, &base, wt)
                        );
                        removed += 1;
                        drop_branch(repo, wt, reason);
                    }
                    Err(e) => {
                        blocked += 1;
                        eprintln!("  kept       {}  ({e})", wt.name());
                    }
                }
            }
        }
    }

    if removed == 0 && blocked == 0 {
        eprintln!("each worktree has work that is not finished");
    } else if dry_run {
        eprintln!("{removed} worktree(s) to remove. wt changed nothing");
    } else if blocked > 0 {
        eprintln!("{removed} worktree(s) removed. {blocked} need(s) your attention");
    } else {
        eprintln!("{removed} worktree(s) removed");
    }
    Ok(())
}

/// Read the facts about one worktree from git.
fn facts(
    repo: &Repo,
    wt: &Worktree,
    base: &str,
    base_local: &str,
    cwd: Option<&Path>,
) -> Result<Facts> {
    let Some(branch) = &wt.branch else {
        return Ok(Facts {
            detached: true,
            ..Facts::default()
        });
    };
    if branch == base_local {
        return Ok(Facts {
            is_base: true,
            ..Facts::default()
        });
    }

    let current = match (cwd, real(wt.path.clone())) {
        // The user can stand in a directory below the worktree.
        (Some(cwd), Some(path)) => cwd.starts_with(&path),
        _ => false,
    };

    let upstream = upstream(repo, branch);

    Ok(Facts {
        is_base: false,
        detached: false,
        on_origin: has_ref(repo, &format!("refs/remotes/origin/{branch}")),
        merged: git::ok(&repo.common, &["merge-base", "--is-ancestor", branch, base]),
        gone: upstream.as_deref().is_some_and(|r| !has_ref(repo, r)),
        // Git does not list the files that it ignores, which is the same
        // rule that `git worktree remove` uses.
        dirty: !git::out(&wt.path, &["status", "--porcelain"])?.is_empty(),
        current,
    })
}

fn has_ref(repo: &Repo, r: &str) -> bool {
    git::ok(&repo.common, &["show-ref", "--verify", "--quiet", r])
}

/// The remote-tracking ref of the upstream branch, for example
/// `refs/remotes/origin/eng-1234`.
///
/// The config holds the upstream branch, and it keeps it after `git fetch
/// --prune` removes the remote-tracking ref. The command reads the config
/// and not `@{upstream}` for that reason: `@{upstream}` fails once the ref
/// is absent, which is exactly the condition that this command looks for.
fn upstream(repo: &Repo, branch: &str) -> Option<String> {
    let remote = git::out(
        &repo.common,
        &["config", "--get", &format!("branch.{branch}.remote")],
    )
    .ok()?;
    let merge = git::out(
        &repo.common,
        &["config", "--get", &format!("branch.{branch}.merge")],
    )
    .ok()?;
    let name = merge.strip_prefix("refs/heads/")?;
    if remote.is_empty() || name.is_empty() {
        return None;
    }
    Some(format!("refs/remotes/{remote}/{name}"))
}

fn describe(reason: Reason, base: &str, wt: &Worktree) -> String {
    match reason {
        Reason::Merged => format!("{base} contains it"),
        Reason::Gone => match &wt.branch {
            Some(b) => format!("origin no longer has {b}"),
            None => "origin no longer has its branch".to_string(),
        },
    }
}

/// Remove the worktree. The error message is the message git gives.
fn remove(repo: &Repo, wt: &Worktree, force: bool) -> Result<()> {
    let path = wt.path.to_string_lossy().into_owned();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&path);
    match git::out(&repo.common, &args) {
        Ok(_) => Ok(()),
        Err(e) => {
            // git puts its whole command line in the message. Keep the last
            // line, which is the reason.
            let text = format!("{e:#}");
            let reason = text.lines().next_back().unwrap_or(&text).trim();
            anyhow::bail!("{reason}")
        }
    }
}

/// Delete the branch of a worktree that the command removed.
///
/// `git branch -d` refuses a branch that HEAD does not contain. The bare
/// repository has its own HEAD, and a squash merge leaves no ancestor, so
/// the refusal is a usual result here. The command already knows that the
/// work is finished, so it deletes the branch and says that it did.
fn drop_branch(repo: &Repo, wt: &Worktree, reason: Reason) {
    let Some(branch) = &wt.branch else { return };
    if git::out(&repo.common, &["branch", "-d", branch]).is_ok() {
        eprintln!("  deleted    branch {branch}");
        return;
    }
    if git::ok(&repo.common, &["branch", "-D", branch]) {
        match reason {
            Reason::Merged => eprintln!("  deleted    branch {branch} (forced)"),
            Reason::Gone => {
                eprintln!("  deleted    branch {branch} (forced: origin no longer has it)")
            }
        }
    } else {
        eprintln!("  kept       branch {branch} (git did not delete it)");
    }
}

/// Remove the administrative files of a worktree whose directory is absent.
///
/// The command compares the list before with the list after. The option `-v`
/// of git writes its own report, and it writes it to the error output, which
/// this tool reserves for messages in its own form. Git also keeps a worktree
/// that a lock holds, so the comparison is the one report that is true.
fn stale(repo: &Repo) -> Result<()> {
    let before = repo.worktrees()?;
    if before.iter().all(|w| w.path.exists()) {
        return Ok(());
    }
    git::quiet(&repo.common, &["worktree", "prune"]);

    let after = repo.worktrees()?;
    for w in before
        .iter()
        .filter(|w| !after.iter().any(|a| a.path == w.path))
    {
        eprintln!("  pruned     {}  (its directory is absent)", w.name());
    }
    Ok(())
}

/// The path with each link resolved. `git worktree list` gives a real path,
/// and the current directory of the shell can hold a link.
fn real(p: std::path::PathBuf) -> Option<std::path::PathBuf> {
    std::fs::canonicalize(&p).ok().or(Some(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A branch that origin has. Each test below starts here, because a
    /// branch with no proof that it reached origin is never a candidate.
    fn pushed() -> Facts {
        Facts {
            on_origin: true,
            ..Facts::default()
        }
    }

    #[test]
    fn the_tool_keeps_a_branch_that_is_not_finished() {
        assert_eq!(verdict(&pushed(), false), Verdict::Keep);
    }

    #[test]
    fn the_tool_removes_a_branch_that_the_base_branch_contains() {
        let f = Facts {
            merged: true,
            ..pushed()
        };
        assert_eq!(verdict(&f, false), Verdict::Remove(Reason::Merged));
    }

    #[test]
    fn the_tool_removes_a_branch_that_origin_no_longer_has() {
        let f = Facts {
            on_origin: false,
            gone: true,
            ..Facts::default()
        };
        assert_eq!(verdict(&f, false), Verdict::Remove(Reason::Gone));
    }

    #[test]
    fn the_merge_is_the_reason_when_both_facts_are_true() {
        let f = Facts {
            merged: true,
            gone: true,
            ..pushed()
        };
        assert_eq!(verdict(&f, false), Verdict::Remove(Reason::Merged));
    }

    /// `wt add eng-1234` makes a branch that holds no commit of its own, so
    /// the base branch contains it. Origin does not have it, and the command
    /// must leave it alone.
    #[test]
    fn the_tool_keeps_a_new_branch_that_origin_does_not_have() {
        let f = Facts {
            merged: true,
            ..Facts::default()
        };
        assert_eq!(verdict(&f, false), Verdict::Keep);
    }

    #[test]
    fn force_does_not_remove_a_branch_that_origin_does_not_have() {
        let f = Facts {
            merged: true,
            dirty: true,
            ..Facts::default()
        };
        assert_eq!(verdict(&f, true), Verdict::Keep);
    }

    /// A branch that origin lost has no remote-tracking ref, and the config
    /// still names its upstream branch. That config is the proof.
    #[test]
    fn a_branch_that_origin_lost_needs_no_remote_tracking_ref() {
        let f = Facts {
            on_origin: false,
            gone: true,
            ..Facts::default()
        };
        assert_eq!(verdict(&f, false), Verdict::Remove(Reason::Gone));
    }

    #[test]
    fn the_tool_keeps_the_base_branch() {
        let f = Facts {
            is_base: true,
            merged: true,
            gone: true,
            ..pushed()
        };
        assert_eq!(verdict(&f, false), Verdict::Keep);
    }

    #[test]
    fn the_tool_keeps_a_worktree_that_is_on_no_branch() {
        let f = Facts {
            detached: true,
            merged: true,
            ..pushed()
        };
        assert_eq!(verdict(&f, false), Verdict::Keep);
    }

    #[test]
    fn the_tool_keeps_the_worktree_that_the_user_is_in() {
        let f = Facts {
            merged: true,
            current: true,
            ..pushed()
        };
        assert_eq!(
            verdict(&f, false),
            Verdict::Blocked(Reason::Merged, Blocker::Current)
        );
    }

    #[test]
    fn force_does_not_remove_the_worktree_that_the_user_is_in() {
        let f = Facts {
            merged: true,
            current: true,
            dirty: true,
            ..pushed()
        };
        assert_eq!(
            verdict(&f, true),
            Verdict::Blocked(Reason::Merged, Blocker::Current)
        );
    }

    #[test]
    fn the_tool_keeps_a_worktree_that_has_a_modified_file() {
        let f = Facts {
            gone: true,
            dirty: true,
            ..pushed()
        };
        assert_eq!(
            verdict(&f, false),
            Verdict::Blocked(Reason::Gone, Blocker::Dirty)
        );
    }

    #[test]
    fn force_removes_a_worktree_that_has_a_modified_file() {
        let f = Facts {
            gone: true,
            dirty: true,
            ..pushed()
        };
        assert_eq!(verdict(&f, true), Verdict::Remove(Reason::Gone));
    }

    #[test]
    fn a_modified_file_alone_is_not_a_reason_to_remove() {
        let f = Facts {
            dirty: true,
            ..pushed()
        };
        assert_eq!(verdict(&f, true), Verdict::Keep);
    }
}
