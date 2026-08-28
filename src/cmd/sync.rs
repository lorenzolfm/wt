use crate::repo::Repo;
use crate::seed::{seed_entry, Outcome};
use anyhow::Result;

/// Compare each worktree with the manifest and make the links that are
/// absent. You can run this command many times. It prints nothing if each
/// worktree is correct.
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
                        "  skipped    {}/{}  (a different file is present)",
                        wt.name(),
                        entry
                    );
                    skipped += 1;
                }
                Outcome::MissingInStore => {
                    eprintln!("  missing    shared/{entry}  (the manifest gives it, the store does not have it)");
                    skipped += 1;
                }
                Outcome::AlreadyLinked => {}
            }
        }
    }

    if changes == 0 && skipped == 0 {
        eprintln!("each worktree is correct");
    } else if skipped > 0 {
        eprintln!("{changes} link(s) made. {skipped} path(s) need your attention. use --force to replace them");
    }
    Ok(())
}
