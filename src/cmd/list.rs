use crate::hook;
use crate::repo::Repo;
use crate::seed::{State, inspect};
use anyhow::Result;

/// Print each worktree, its branch, and the condition of its links.
pub fn run(repo: &Repo) -> Result<()> {
    let worktrees = repo.worktrees()?;
    if worktrees.is_empty() {
        eprintln!("this repository has no worktree");
        return Ok(());
    }

    let manifest = repo.manifest()?;
    let shared = repo.shared();
    let managed = repo.is_managed() && !manifest.link.is_empty();

    let rows: Vec<(String, String)> = worktrees
        .iter()
        .map(|w| {
            let branch = w.branch.clone().unwrap_or_else(|| "(detached)".to_string());
            (w.name(), branch)
        })
        .collect();

    let name_width = rows
        .iter()
        .map(|(n, _)| n.chars().count())
        .max()
        .unwrap_or(0);
    let branch_width = rows
        .iter()
        .map(|(_, b)| b.chars().count())
        .max()
        .unwrap_or(0);

    for (wt, (name, branch)) in worktrees.iter().zip(&rows) {
        if managed {
            println!(
                "{name:name_width$}  {branch:branch_width$}  {}",
                links(&shared, wt, &manifest.link)
            );
        } else {
            println!("{name:name_width$}  {branch}");
        }
    }

    if let Some(warning) = hook::check(repo) {
        eprintln!("warning: {warning}");
    }
    Ok(())
}

/// Count the links that are correct, and name the paths that are not.
///
/// This function only reports. `inspect` changes nothing.
fn links(shared: &std::path::Path, wt: &crate::repo::Worktree, entries: &[String]) -> String {
    let mut good = 0usize;
    let mut problems = Vec::new();

    for entry in entries {
        match inspect(shared, &wt.path, entry) {
            Ok(State::Linked) => good += 1,
            Ok(State::Absent) => problems.push(format!("{entry} absent")),
            Ok(State::WrongLink) => problems.push(format!("{entry} points elsewhere")),
            Ok(State::SameFile) => problems.push(format!("{entry} is a copy")),
            Ok(State::DifferentFile) => problems.push(format!("{entry} differs")),
            Ok(State::MissingInStore) => problems.push(format!("{entry} not in store")),
            Err(e) => problems.push(format!("{entry}: {e}")),
        }
    }

    if problems.is_empty() {
        format!("{good}/{} links", entries.len())
    } else {
        format!("{good}/{} links  ({})", entries.len(), problems.join(", "))
    }
}
