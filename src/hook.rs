use crate::repo::Repo;
use crate::seed::{self, Outcome};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

/// The paths a hook is permitted to point at.
///
/// A hook must point to a path that does not change. Nix profile and system
/// paths are symbolic links that nix updates in place, so they are safe.
fn stable_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        paths.push(home.join(".local").join("bin").join("wt"));
        paths.push(home.join(".nix-profile").join("bin").join("wt"));
    }
    paths.push(PathBuf::from("/run/current-system/sw/bin/wt"));
    paths
}

/// Select the path to write into the hook link.
///
/// A hook stays in the repository for longer than the program that made it.
/// On NixOS, `current_exe()` gives a `/nix/store/<hash>` path. That hash
/// changes with each build, and nix removes the old path. Git does not show
/// an error for a hook that it cannot run. The seed operation stops, and the
/// user does not see a message. Therefore, prefer a path that does not change.
pub fn install_target() -> Result<PathBuf> {
    for candidate in stable_candidates() {
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    let exe = std::env::current_exe().context("cannot find the path of this program")?;
    if exe.starts_with("/nix/store") {
        bail!(
            "the program is at {}\n  \
             a git hook must not point to a /nix/store path: the path changes with each\n  \
             build, and git ignores a hook that it cannot run. make a stable path first:\n    \
             ln -s {} ~/.local/bin/wt\n  \
             or install the program with `nix profile install`",
            exe.display(),
            exe.display()
        );
    }
    Ok(exe)
}

/// Make the `post-checkout` hook, or point it to a new path. Return the
/// target path, and a flag that shows a change.
pub fn install(repo: &Repo) -> Result<(PathBuf, bool)> {
    let target = install_target()?;
    let hook = repo.hook_path();
    if let Some(parent) = hook.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Ok(existing) = fs::read_link(&hook)
        && existing == target
    {
        return Ok((target, false));
    }
    if fs::symlink_metadata(&hook).is_ok() {
        fs::remove_file(&hook)
            .with_context(|| format!("wt cannot replace the hook at {}", hook.display()))?;
    }
    std::os::unix::fs::symlink(&target, &hook)
        .with_context(|| format!("wt cannot make the hook at {}", hook.display()))?;
    Ok((target, true))
}

/// Examine the hook. This function changes a failure that the user cannot
/// see into a message that the user can see.
pub fn check(repo: &Repo) -> Option<String> {
    let hook = repo.hook_path();
    if fs::symlink_metadata(&hook).is_err() {
        return Some(format!(
            "the post-checkout hook is not at {}. wt cannot make the links automatically. run `wt init`",
            hook.display()
        ));
    }
    if !hook.exists() {
        let target = fs::read_link(&hook)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "?".into());
        return Some(format!(
            "the post-checkout hook points to {target}, and that path is not present\n  git ignores the hook and shows no error. run `wt init` to correct it"
        ));
    }
    None
}

/// Git runs this function as the `post-checkout` hook. The current directory
/// is the new worktree.
///
/// The function always returns 0. Git also runs the hook for each
/// `git switch`. A problem in the seed operation must not show a different
/// git command as a failure.
pub fn run_as_hook(args: &[String]) -> i32 {
    // args: <prev-head> <new-head> <branch-checkout-flag>
    let is_creation = args
        .first()
        .map(|s| !s.is_empty() && s.chars().all(|c| c == '0'))
        .unwrap_or(false);
    if !is_creation {
        return 0;
    }
    if let Err(e) = seed_here() {
        eprintln!("wt: {e:#}");
    }
    0
}

fn seed_here() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let repo = Repo::discover(Some(&cwd))?;
    if !repo.is_managed() {
        return Ok(());
    }
    let manifest = repo.manifest()?;
    if manifest.link.is_empty() {
        return Ok(());
    }
    report(&repo.shared(), &cwd, &manifest.link)
}

fn report(shared: &Path, worktree: &Path, entries: &[String]) -> Result<()> {
    let mut linked = 0usize;
    for entry in entries {
        match seed::seed_entry(shared, worktree, entry, false)? {
            o if o.is_change() => linked += 1,
            Outcome::SkippedDivergent => {
                eprintln!("wt: {entry}: a different file is present. wt kept it")
            }
            Outcome::MissingInStore => {
                eprintln!("wt: {entry}: the config gives this path, the store does not have it")
            }
            _ => {}
        }
    }
    if linked > 0 {
        eprintln!("wt: made {linked} link(s)");
    }
    Ok(())
}
