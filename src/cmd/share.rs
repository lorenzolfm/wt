use crate::git;
use crate::repo::Repo;
use crate::seed::{seed_entry, Outcome};
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Move gitignored files into the store and link them back everywhere.
pub fn run(repo: &Repo, paths: &[String], force: bool) -> Result<()> {
    repo.require_managed()?;
    let cwd = std::env::current_dir()?;
    let toplevel = PathBuf::from(git::out(&cwd, &["rev-parse", "--show-toplevel"])?);
    let mut manifest = repo.manifest()?;

    for raw in paths {
        match share_one(repo, &toplevel, &cwd, raw, &mut manifest, force) {
            Ok(()) => {}
            Err(e) => eprintln!("error: {raw}: {e:#}"),
        }
    }

    manifest.save(&repo.manifest_path())?;
    Ok(())
}

fn share_one(
    repo: &Repo,
    toplevel: &Path,
    cwd: &Path,
    raw: &str,
    manifest: &mut crate::manifest::Manifest,
    force: bool,
) -> Result<()> {
    let entry = relative_entry(toplevel, cwd, raw)?;
    let source = toplevel.join(&entry);
    let stored = repo.shared().join(&entry);

    if manifest.contains(&entry) {
        bail!("already shared (in the manifest)");
    }

    // Tracked paths are delivered by git itself; sharing them would shadow
    // the checkout. This also catches `wt share terraform` when only
    // terraform/terraform.tfvars is ignored.
    let tracked = git::out(toplevel, &["ls-files", "--", &entry])?;
    if !tracked.is_empty() {
        let first = tracked.lines().next().unwrap_or_default();
        bail!(
            "contains tracked files (e.g. {first})\n  \
             git already checks those out; share the ignored leaf instead"
        );
    }

    if !git::ok(toplevel, &["check-ignore", "-q", "--", &entry]) {
        bail!("not gitignored -- wt only manages ignored paths");
    }

    if !source.exists() {
        bail!("no such path in {}", toplevel.display());
    }
    if fs::symlink_metadata(&source)?.file_type().is_symlink() {
        bail!("already a symlink");
    }
    if stored.exists() {
        bail!("shared/{entry} already exists in the store");
    }

    if let Some(parent) = stored.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(&source, &stored)
        .with_context(|| format!("moving {} into the store", source.display()))?;
    eprintln!("  moved      {} -> shared/{}", display_rel(toplevel, &source), entry);

    manifest.insert(&entry);

    // Backfill every worktree, this one included.
    let shared = repo.shared();
    let mut replaced = Vec::new();
    for wt in repo.worktrees()? {
        match seed_entry(&shared, &wt.path, &entry, force)? {
            Outcome::Linked | Outcome::Replaced | Outcome::Forced => replaced.push(wt.name()),
            Outcome::SkippedDivergent => eprintln!(
                "  skipped    {}  (real file, differs from store -- --force to overwrite)",
                wt.name()
            ),
            Outcome::MissingInStore | Outcome::AlreadyLinked => {}
        }
    }
    if !replaced.is_empty() {
        eprintln!("  linked     {}", replaced.join(", "));
    }
    Ok(())
}

/// Normalise a user-supplied path to a worktree-relative entry.
fn relative_entry(toplevel: &Path, cwd: &Path, raw: &str) -> Result<String> {
    let joined = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        cwd.join(raw)
    };
    // Resolve `.` / `..` without requiring the path to exist.
    let mut normal = PathBuf::new();
    for part in joined.components() {
        match part {
            std::path::Component::ParentDir => {
                normal.pop();
            }
            std::path::Component::CurDir => {}
            other => normal.push(other),
        }
    }
    let rel = normal
        .strip_prefix(toplevel)
        .with_context(|| format!("{raw} is outside the worktree at {}", toplevel.display()))?;
    if rel.as_os_str().is_empty() {
        bail!("refusing to share the worktree root");
    }
    Ok(rel.to_string_lossy().into_owned())
}

fn display_rel(base: &Path, p: &Path) -> String {
    p.strip_prefix(base)
        .unwrap_or(p)
        .to_string_lossy()
        .into_owned()
}
