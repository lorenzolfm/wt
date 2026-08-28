use crate::hook;
use crate::manifest::Manifest;
use crate::repo::Repo;
use anyhow::Result;
use std::fs;

/// Pure scaffolding: the store, the manifest, the hook. Never moves files.
pub fn run(repo: &Repo) -> Result<()> {
    let mut changed = false;

    let shared = repo.shared();
    if !shared.exists() {
        fs::create_dir_all(&shared)?;
        eprintln!("  created    shared/");
        changed = true;
    }

    let manifest_path = repo.manifest_path();
    if !manifest_path.exists() {
        Manifest::default().save(&manifest_path)?;
        eprintln!("  created    worktree-shared.toml");
        changed = true;
    }

    let (target, hook_changed) = hook::install(repo)?;
    if hook_changed {
        eprintln!("  installed  hooks/post-checkout -> {}", target.display());
        changed = true;
    }

    if changed {
        eprintln!("initialised {}", repo.common.display());
    } else {
        eprintln!("{} is already initialised", repo.common.display());
    }
    Ok(())
}
