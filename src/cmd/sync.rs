use crate::repo::Repo;
use crate::seed::{seed_entry, Outcome};
use anyhow::Result;

/// Reconcile every worktree against the manifest. Idempotent; quiet when clean.
pub fn run(repo: &Repo, force: bool) -> Result<()> {
    repo.require_managed()?;
    if let Some(w) = crate::hook::check(repo) {
        eprintln!("warning: {w}");
    }

    let manifest = repo.manifest()?;
    let shared = repo.shared();
    let mut changes = 0usize;
    let mut skipped = 0usize;

    for wt in repo.worktrees()? {
        for entry in &manifest.link {
            let outcome = seed_entry(&shared, &wt.path, entry, force)?;
            match outcome {
                Outcome::Linked | Outcome::Replaced | Outcome::Forced => {
                    eprintln!("  linked     {}/{}", wt.name(), entry);
                    changes += 1;
                }
                Outcome::SkippedDivergent => {
                    eprintln!(
                        "  skipped    {}/{}  (real file, differs from store)",
                        wt.name(),
                        entry
                    );
                    skipped += 1;
                }
                Outcome::MissingInStore => {
                    eprintln!("  missing    shared/{entry}  (listed in manifest, absent from store)");
                    skipped += 1;
                }
                Outcome::AlreadyLinked => {}
            }
        }
    }

    if changes == 0 && skipped == 0 {
        eprintln!("everything in sync");
    } else if skipped > 0 {
        eprintln!("{changes} linked, {skipped} needing attention (--force to overwrite)");
    }
    Ok(())
}
