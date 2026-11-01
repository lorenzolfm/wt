use crate::repo::Repo;
use crate::seed::{Outcome, seed_entry};
use anyhow::Result;

/// Compare each worktree with the config and make the links that are
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
    let mut below_link = 0usize;

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
                    eprintln!(
                        "  missing    shared/{entry}  (the config gives it, the store does not have it)"
                    );
                    skipped += 1;
                }
                Outcome::SkippedBelowLink => {
                    eprintln!(
                        "  skipped    {}/{}  (a directory above it is a link)",
                        wt.name(),
                        entry
                    );
                    below_link += 1;
                }
                Outcome::AlreadyLinked => {}
            }
        }
    }

    if changes == 0 && skipped == 0 && below_link == 0 {
        eprintln!("each worktree is correct");
    } else if skipped > 0 {
        eprintln!(
            "{changes} link(s) made. {skipped} path(s) need your attention. use --force to replace them"
        );
    }
    // --force does not change a path below a link.
    if below_link > 0 {
        eprintln!(
            "{below_link} path(s) are below a link, and wt changes nothing there. remove the link, or remove the path from the config"
        );
    }
    Ok(())
}
