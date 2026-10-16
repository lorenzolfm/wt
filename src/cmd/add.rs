use crate::config::key_from_url;
use crate::git;
use crate::hook;
use crate::repo::Repo;
use anyhow::{Result, bail};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fetch {
    /// Fetch only if the branch is not known. This is the default.
    Lazy,
    /// Do not use the network.
    Never,
    /// Fetch before the tool finds the branch.
    Always,
}

/// Make a worktree for `branch`.
///
/// `branch` can also be the link to a pull request on GitHub. The command
/// then uses the branch of the pull request, and the result is the same as
/// for the branch name.
///
/// The command finds the branch in this sequence: a local branch, then a
/// remote branch, then a fetch from the network, then a new branch.
///
/// A branch can be in one worktree only. When a worktree has `branch`
/// already, the command prints the path of that worktree, so the shell goes
/// there.
pub fn run(repo: &Repo, branch: &str, dir: Option<&str>, fetch: Fetch) -> Result<()> {
    if let Some(w) = hook::check(repo) {
        eprintln!("warning: {w}");
    }

    let link = is_link(branch);
    let branch = &branch_of(repo, branch, fetch)?;

    // Origin has the branch of a pull request with the same name. A local
    // branch can track a different branch, for example master when it
    // started from origin/master. `git push` then refuses to push it.
    if link && has_local(repo, branch) {
        track_origin(repo, branch)?;
    }

    if let Some(w) = repo
        .worktrees()?
        .into_iter()
        .find(|w| w.branch.as_deref() == Some(branch))
    {
        eprintln!("  {}  (branch {branch} is here already)", w.name());
        println!("{}", w.path.display());
        return Ok(());
    }

    let name = dir
        .map(str::to_string)
        .unwrap_or_else(|| branch.replace('/', "-"));
    let dest: PathBuf = repo.container().join(&name);
    if dest.exists() {
        bail!("{} is already present", dest.display());
    }
    let dest_str = dest.to_string_lossy().into_owned();
    let remote_ref = format!("origin/{branch}");

    if fetch == Fetch::Always {
        eprintln!("  fetching   origin {branch}");
        git::quiet(&repo.common, &["fetch", "origin", branch]);
    }

    let args: Vec<String> = if has_local(repo, branch) {
        eprintln!("  branch     {branch} (local)");
        vec!["worktree".into(), "add".into(), dest_str, branch.into()]
    } else if has_remote(repo, &remote_ref) {
        eprintln!("  branch     {branch} (tracks {remote_ref})");
        track_args(&dest_str, branch, &remote_ref)
    } else {
        // A branch that is not present is the usual condition here. A failed
        // fetch is therefore an expected result, and its output is not useful.
        let found_remotely = fetch != Fetch::Never && {
            git::quiet(&repo.common, &["fetch", "origin", branch]);
            has_remote(repo, &remote_ref)
        };

        if found_remotely {
            eprintln!("  branch     {branch} (tracks {remote_ref}, from a new fetch)");
            track_args(&dest_str, branch, &remote_ref)
        } else {
            let base = repo.default_base()?;
            eprintln!("  branch     {branch} (new branch from {base})");
            // Use --no-track. A new branch from origin/master must not keep
            // master as its upstream branch. If it does, `git push` sends the
            // commits to master.
            vec![
                "worktree".into(),
                "add".into(),
                "--no-track".into(),
                "-b".into(),
                branch.into(),
                dest_str,
                base,
            ]
        }
    };

    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    git::passthrough(&repo.common, &argv)?;

    if crate::cmd::is_command(&name) {
        eprintln!("warning: wt {name} runs the command {name}, not this worktree");
        eprintln!("  use wt cd {name} to go to it");
    }

    // The hook makes the links. The output gives the path to the shell.
    println!("{}", dest.display());
    Ok(())
}

/// The branch for `arg`. A branch name comes back unchanged. A link to a pull
/// request gives the branch of that pull request.
///
/// Git has no ref for the branch name of a pull request. Origin has
/// `refs/pull/<number>/head`, which holds the commit. The function asks
/// origin for that ref and for each branch, and it finds the branch at that
/// commit.
fn branch_of(repo: &Repo, arg: &str, fetch: Fetch) -> Result<String> {
    if !is_link(arg) {
        return Ok(arg.to_string());
    }
    let Some((key, number)) = pull_request(arg) else {
        bail!("{arg} is not a link to a pull request");
    };
    let origin = repo.key()?;
    if key != origin {
        bail!("pull request {number} is in {key}, and origin is {origin}");
    }
    if fetch == Fetch::Never {
        bail!("pull request {number} needs the network to find its branch. remove --no-fetch");
    }

    eprintln!("  pull       #{number} (asking origin for its branch)");
    let pull_ref = format!("refs/pull/{number}/head");
    let text = git::out(
        &repo.common,
        &["ls-remote", "origin", &pull_ref, "refs/heads/*"],
    )?;
    match heads_at(&text, &pull_ref).as_deref() {
        None => bail!("origin has no pull request {number}"),
        Some([]) => bail!(
            "origin has no branch at the head of pull request {number}\n  \
             the pull request can come from a fork, or origin deleted its branch"
        ),
        Some([branch]) => Ok(branch.clone()),
        Some(many) => bail!(
            "more than one branch is at the head of pull request {number}: {}\n  \
             use wt add <branch>",
            many.join(", ")
        ),
    }
}

fn is_link(arg: &str) -> bool {
    // Git does not permit ':' in a branch name, so a link is never a branch.
    arg.contains("://")
}

/// Make `origin/<branch>` the upstream branch of the local `branch`. Change
/// nothing if it is the upstream branch already.
///
/// The remote-tracking ref must be present. `git branch --set-upstream-to`
/// refuses a ref that is absent, and `wt prune` reads an upstream branch
/// without its ref as a branch that origin deleted. The function therefore
/// fetches the branch when the ref is absent.
fn track_origin(repo: &Repo, branch: &str) -> Result<()> {
    let remote_ref = format!("origin/{branch}");
    let was = repo.upstream(branch);
    if was.as_deref() == Some(&format!("refs/remotes/{remote_ref}")) {
        return Ok(());
    }

    if !has_remote(repo, &remote_ref) {
        eprintln!("  fetching   origin {branch}");
        git::quiet(&repo.common, &["fetch", "origin", branch]);
    }
    git::out(
        &repo.common,
        &["branch", "--set-upstream-to", &remote_ref, branch],
    )?;

    let was = was
        .as_deref()
        .map(|r| r.strip_prefix("refs/remotes/").unwrap_or(r))
        .unwrap_or("none");
    eprintln!("  upstream   {remote_ref} (was {was})");
    Ok(())
}

/// The repository key and the number of a pull request, from its link.
///
///   https://github.com/owner/repo/pull/123        -> (github.com/owner/repo, 123)
///   https://github.com/owner/repo/pull/123/files  -> (github.com/owner/repo, 123)
fn pull_request(link: &str) -> Option<(String, u64)> {
    let (repo, rest) = link.split_once("/pull/")?;
    let number = rest.split(['/', '?', '#']).next()?.parse().ok()?;
    Some((key_from_url(repo), number))
}

/// The branches at the commit of `pull_ref`. `listing` is the output of
/// `git ls-remote`. The result is `None` when the listing does not have
/// `pull_ref`.
fn heads_at(listing: &str, pull_ref: &str) -> Option<Vec<String>> {
    let refs: Vec<(&str, &str)> = listing
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .collect();
    let (commit, _) = refs.iter().find(|(_, name)| *name == pull_ref)?;
    Some(
        refs.iter()
            .filter(|(c, _)| c == commit)
            .filter_map(|(_, name)| name.strip_prefix("refs/heads/"))
            .map(str::to_string)
            .collect(),
    )
}

fn track_args(dest: &str, branch: &str, remote_ref: &str) -> Vec<String> {
    vec![
        "worktree".into(),
        "add".into(),
        "--track".into(),
        "-b".into(),
        branch.into(),
        dest.into(),
        remote_ref.into(),
    ]
}

fn has_local(repo: &Repo, branch: &str) -> bool {
    git::ok(
        &repo.common,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
}

fn has_remote(repo: &Repo, remote_ref: &str) -> bool {
    git::ok(
        &repo.common,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/remotes/{remote_ref}"),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn a_link_gives_the_repository_and_the_number() {
        for link in [
            "https://github.com/owner/repo/pull/123",
            "https://github.com/owner/repo/pull/123/",
            "https://github.com/owner/repo/pull/123/files",
            "https://github.com/owner/repo/pull/123?w=1",
            "https://github.com/owner/repo/pull/123#issuecomment-1",
        ] {
            assert_eq!(
                pull_request(link),
                Some(("github.com/owner/repo".to_string(), 123)),
                "for {link}"
            );
        }
    }

    #[test]
    fn a_link_to_a_different_page_is_not_a_pull_request() {
        for link in [
            "https://github.com/owner/repo",
            "https://github.com/owner/repo/pulls",
            "https://github.com/owner/repo/issues/123",
            "https://github.com/owner/repo/pull/abc",
        ] {
            assert_eq!(pull_request(link), None, "for {link}");
        }
    }

    const LISTING: &str = "\
aaa\trefs/heads/main
bbb\trefs/heads/lorenzo/eng-1234
ccc\trefs/heads/eng-5678
bbb\trefs/pull/7/head
";

    #[test]
    fn the_branch_is_the_one_at_the_commit_of_the_pull_request() {
        assert_eq!(
            heads_at(LISTING, "refs/pull/7/head"),
            Some(vec!["lorenzo/eng-1234".to_string()])
        );
    }

    #[test]
    fn a_pull_request_that_origin_does_not_have_gives_no_result() {
        assert_eq!(heads_at(LISTING, "refs/pull/8/head"), None);
    }

    /// Run git for a fixture. The fixture ignores the config of the user.
    fn git(dir: &Path, args: &[&str]) {
        let o = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "wt")
            .env("GIT_AUTHOR_EMAIL", "wt@example.com")
            .env("GIT_COMMITTER_NAME", "wt")
            .env("GIT_COMMITTER_EMAIL", "wt@example.com")
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr)
        );
    }

    /// `git worktree add -b feature <path> origin/master` gives a branch that
    /// tracks master. The repository has no remote-tracking ref for the branch
    /// of the pull request yet.
    #[test]
    fn the_branch_of_a_pull_request_tracks_its_branch_on_origin() {
        let tmp = TempDir::new().unwrap();
        let origin = tmp.path().join("origin");
        let bare = tmp.path().join(".bare");
        git(
            tmp.path(),
            &["init", "-b", "master", origin.to_str().unwrap()],
        );
        git(&origin, &["commit", "--allow-empty", "-m", "one"]);
        git(&origin, &["branch", "feature"]);
        git(tmp.path(), &["init", "--bare", bare.to_str().unwrap()]);
        git(
            &bare,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        git(&bare, &["fetch", "origin", "master"]);
        git(&bare, &["branch", "--track", "feature", "origin/master"]);
        let repo = Repo { common: bare };

        track_origin(&repo, "feature").unwrap();

        assert_eq!(
            repo.upstream("feature").as_deref(),
            Some("refs/remotes/origin/feature")
        );
        assert!(has_remote(&repo, "origin/feature"));
    }
}
