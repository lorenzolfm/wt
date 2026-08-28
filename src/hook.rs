use crate::repo::Repo;
use crate::seed::{self, Outcome};
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// The stable indirection a hook is allowed to point at.
fn stable_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("bin").join("wt"))
}

/// Decide what path to write into a hook symlink.
///
/// A hook outlives the binary that installed it. On NixOS `current_exe()` is
/// a `/nix/store/<hash>` path that changes on every rebuild and is eventually
/// garbage-collected -- and git skips a dangling hook *silently*, so seeding
/// would stop working with no error anywhere. Prefer a stable indirection.
pub fn install_target() -> Result<PathBuf> {
    if let Some(stable) = stable_path() {
        if stable.exists() {
            return Ok(stable);
        }
    }
    let exe = std::env::current_exe().context("resolving own path")?;
    if exe.starts_with("/nix/store") {
        bail!(
            "refusing to point a git hook at {}\n  \
             that path changes on every rebuild and is garbage-collected, which would\n  \
             break seeding silently. create a stable indirection first:\n    \
             ln -s {} ~/.local/bin/wt",
            exe.display(),
            exe.display()
        );
    }
    Ok(exe)
}

/// Install (or repoint) the post-checkout hook. Returns the target and
/// whether anything changed.
pub fn install(repo: &Repo) -> Result<(PathBuf, bool)> {
    let target = install_target()?;
    let hook = repo.hook_path();
    if let Some(parent) = hook.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Ok(existing) = fs::read_link(&hook) {
        if existing == target {
            return Ok((target, false));
        }
    }
    if fs::symlink_metadata(&hook).is_ok() {
        fs::remove_file(&hook)
            .with_context(|| format!("replacing existing hook at {}", hook.display()))?;
    }
    std::os::unix::fs::symlink(&target, &hook)
        .with_context(|| format!("installing hook at {}", hook.display()))?;
    Ok((target, true))
}

/// One stat, turning a silent failure into a visible one.
pub fn check(repo: &Repo) -> Option<String> {
    let hook = repo.hook_path();
    if fs::symlink_metadata(&hook).is_err() {
        return Some(format!(
            "no post-checkout hook at {} -- worktrees will not be seeded automatically (`wt init`)",
            hook.display()
        ));
    }
    if !hook.exists() {
        let target = fs::read_link(&hook)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "?".into());
        return Some(format!(
            "post-checkout hook dangles -> {target}\n  git skips it silently; seeding is disabled (`wt init` to repoint)"
        ));
    }
    None
}

/// Invoked as `post-checkout` by git, with cwd set to the worktree.
///
/// Always exits 0: this runs on every `git switch` too, and a seeding
/// problem should never make an unrelated git command look like it failed.
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
                eprintln!("wt: {entry}: local copy differs from the store, left in place")
            }
            Outcome::MissingInStore => {
                eprintln!("wt: {entry}: listed in the manifest but missing from the store")
            }
            _ => {}
        }
    }
    if linked > 0 {
        eprintln!("wt: seeded {linked} shared path(s)");
    }
    Ok(())
}
