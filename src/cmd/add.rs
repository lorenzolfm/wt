use crate::config::RepoKey;
use crate::git;
use crate::hook;
use crate::repo::Repo;
use anyhow::{Context, Result, bail};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fetch {
    /// Fetch only if the branch is not known. This is the default.
    Lazy,
    /// Do not use the network.
    Never,
    /// Fetch before the tool finds the branch.
    Always,
}

/// The name of a worktree directory. It is one component of a path, so the
/// worktree is beside the other worktrees, and `wt cd <name>` can reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirName(String);

impl DirName {
    pub fn parse(name: &str) -> Result<DirName> {
        let mut parts = Path::new(name).components();
        match (parts.next(), parts.next()) {
            (Some(Component::Normal(n)), None) if n == name => Ok(DirName(name.to_string())),
            _ => bail!("{name} is not a directory name. a name has no '/', and it is not . or .."),
        }
    }
}

/// The worktree that has the branch.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The command made the worktree.
    Made(PathBuf),
    /// A worktree had the branch already. The command ignored `dir`.
    Present(PathBuf),
}

impl Outcome {
    pub fn path(&self) -> &Path {
        match self {
            Outcome::Made(p) | Outcome::Present(p) => p,
        }
    }
}

/// Make a worktree for `branch`.
///
/// `branch` can also be the link to a pull request on GitHub. The command
/// then uses the branch of the pull request, and the result is the same as
/// for the branch name. The branch of a pull request from a fork tracks the
/// branch in the fork.
///
/// The command finds the branch in this sequence: a local branch, then a
/// remote branch, then a fetch from the network, then a new branch.
///
/// The result is the worktree. A branch can be in one worktree only. When a
/// worktree has `branch` already, the result is that worktree, so the shell
/// goes there.
///
/// The command refuses a name that is not a valid branch name, also when
/// GitHub gives it for a pull request.
///
/// The progress goes to stderr. The function prints nothing on stdout.
pub fn run(repo: &Repo, branch: &str, dir: Option<&DirName>, fetch: Fetch) -> Result<Outcome> {
    if let Some(w) = hook::check(repo) {
        eprintln!("warning: {w}");
    }

    let link = is_link(branch);
    let b = branch_of(repo, branch, fetch)?;
    make(repo, &b, link, dir, fetch)
}

/// A local branch and the remote branch that it tracks.
#[derive(Debug, PartialEq, Eq)]
struct Branch {
    /// The name of the local branch.
    name: String,
    /// The remote that has the branch: origin, or the remote of a fork.
    remote: String,
    /// The name of the branch on the remote. It is different from `name`
    /// only when a different local branch has that name.
    upstream: String,
}

impl Branch {
    fn on_origin(name: &str) -> Branch {
        Branch {
            name: name.to_string(),
            remote: "origin".to_string(),
            upstream: name.to_string(),
        }
    }

    /// The remote-tracking branch, for example `origin/eng-1234`.
    fn remote_ref(&self) -> String {
        format!("{}/{}", self.remote, self.upstream)
    }
}

/// Make a worktree for `b`. For a link, the remote branch must be present:
/// the command does not make a new branch for a pull request.
fn make(
    repo: &Repo,
    b: &Branch,
    link: bool,
    dir: Option<&DirName>,
    fetch: Fetch,
) -> Result<Outcome> {
    // Git reads more than a name in these. `git fetch` reads a refspec, and
    // `git worktree add -b` gives the name to `git branch` with no `--`.
    for name in [&b.name, &b.upstream] {
        if !git::is_branch_name(&repo.common, name) {
            bail!("{name} is not a valid branch name");
        }
    }
    let branch = &b.name;

    // A local branch can track a different branch, for example master when
    // it started from origin/master. `git push` then refuses to push it.
    if link && has_local(repo, branch) {
        track(repo, b)?;
    }

    if let Some(w) = repo
        .worktrees()?
        .into_iter()
        .find(|w| w.branch.as_deref() == Some(branch))
    {
        eprintln!("  {}  (branch {branch} is here already)", w.name());
        return Ok(Outcome::Present(w.path));
    }

    let name = match dir {
        Some(d) => d.clone(),
        None => DirName::parse(&branch.replace('/', "-"))?,
    };
    let dest: PathBuf = repo.container().join(&name.0);
    if dest.exists() {
        bail!("{} is already present", dest.display());
    }
    let dest_str = dest.to_string_lossy().into_owned();
    let remote_ref = b.remote_ref();

    if fetch == Fetch::Always {
        eprintln!("  fetching   {} {}", b.remote, b.upstream);
        fetch_branch(repo, b);
    }

    let args: Vec<String> = if has_local(repo, branch) {
        eprintln!("  branch     {branch} (local)");
        vec![
            "worktree".into(),
            "add".into(),
            "--".into(),
            dest_str,
            branch.into(),
        ]
    } else if has_remote(repo, &remote_ref) {
        eprintln!("  branch     {branch} (tracks {remote_ref})");
        track_args(&dest_str, branch, &remote_ref)
    } else {
        // A branch that is not present is the usual condition here. A failed
        // fetch is therefore an expected result, and its output is not useful.
        let found_remotely = fetch != Fetch::Never && {
            fetch_branch(repo, b);
            has_remote(repo, &remote_ref)
        };

        if found_remotely {
            eprintln!("  branch     {branch} (tracks {remote_ref}, from a new fetch)");
            track_args(&dest_str, branch, &remote_ref)
        } else if link {
            // A new branch would not have the commits of the pull request,
            // and the worktree would look correct.
            bail!(
                "wt cannot fetch {remote_ref}\n  run git fetch -- {} {} to see why",
                b.remote,
                b.upstream
            );
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
                "--".into(),
                dest_str,
                base,
            ]
        }
    };

    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    // The hook makes the links.
    git::passthrough(&repo.common, &argv)?;
    Ok(Outcome::Made(dest))
}

/// The branch for `arg`. A branch name gives that branch on origin. A link
/// to a pull request gives the branch of that pull request.
///
/// Git has no ref for the branch name of a pull request. Origin has
/// `refs/pull/<number>/head`, which holds the commit. The function asks
/// origin for that ref and for each branch, and it finds the branch at that
/// commit. When origin has no branch at that commit, the pull request can
/// come from a fork, and the function asks GitHub for the fork.
///
/// The link can also point to the repository that origin is a fork of.
fn branch_of(repo: &Repo, arg: &str, fetch: Fetch) -> Result<Branch> {
    if !is_link(arg) {
        return Ok(Branch::on_origin(arg));
    }
    let Some((key, number)) = pull_request(arg) else {
        bail!("{arg} is not a link to a pull request");
    };
    if fetch == Fetch::Never {
        bail!("pull request {number} needs the network to find its branch. remove --no-fetch");
    }
    let origin = repo.key()?;
    if key != origin {
        return parent_branch(repo, &key, &origin, number);
    }

    eprintln!("  pull       #{number} (asking origin for its branch)");
    let pull_ref = format!("refs/pull/{number}/head");
    let text = git::out(
        &repo.common,
        &["ls-remote", "origin", &pull_ref, "refs/heads/*"],
    )?;
    match heads_at(&text, &pull_ref).as_deref() {
        None => bail!("origin has no pull request {number}"),
        Some([]) => {
            eprintln!("  pull       #{number} (asking GitHub for its fork)");
            let head = head_of(&key, number)?;
            if head.key() == origin {
                bail!(
                    "origin no longer has {}, the branch of pull request {number}",
                    head.branch
                );
            }
            fork_branch(repo, &head)
        }
        Some([branch]) => Ok(Branch::on_origin(branch)),
        Some(many) => bail!(
            "more than one branch is at the head of pull request {number}: {}\n  \
             use wt add <branch>",
            many.join(", ")
        ),
    }
}

/// The branch of a pull request in `key`, the repository that origin is a
/// fork of. Origin has no `refs/pull/<number>/head` for it, so the function
/// asks GitHub for the head. The head can be in origin, in `key` or in a
/// different fork.
///
/// The function refuses a repository that origin is not a fork of. Its
/// branch has a different history, and a fetch can be large.
fn parent_branch(repo: &Repo, key: &RepoKey, origin: &RepoKey, number: u64) -> Result<Branch> {
    eprintln!("  pull       #{number} (asking GitHub for its fork)");
    if !forked_from(origin)?.contains(key) {
        bail!("pull request {number} is in {key}, and origin {origin} is not a fork of it");
    }
    fork_branch(repo, &head_of(key, number)?)
}

fn is_link(arg: &str) -> bool {
    // Git does not permit ':' in a branch name, so a link is never a branch.
    arg.contains("://")
}

/// Make the branch on the remote the upstream branch of the local branch.
/// Change nothing if it is the upstream branch already.
///
/// The remote-tracking ref must be present. `git branch --set-upstream-to`
/// refuses a ref that is absent, and `wt prune` reads an upstream branch
/// without its ref as a branch that origin deleted. The function therefore
/// fetches the branch when the ref is absent.
fn track(repo: &Repo, b: &Branch) -> Result<()> {
    let remote_ref = b.remote_ref();
    let was = repo.upstream(&b.name);
    if was.as_deref() == Some(&format!("refs/remotes/{remote_ref}")) {
        return Ok(());
    }

    if !has_remote(repo, &remote_ref) {
        eprintln!("  fetching   {} {}", b.remote, b.upstream);
        fetch_branch(repo, b);
    }
    git::out(
        &repo.common,
        &["branch", "--set-upstream-to", &remote_ref, "--", &b.name],
    )?;

    let was = was
        .as_deref()
        .map(|r| r.strip_prefix("refs/remotes/").unwrap_or(r))
        .unwrap_or("none");
    eprintln!("  upstream   {remote_ref} (was {was})");
    Ok(())
}

/// The head of a pull request, as GitHub gives it.
#[derive(Debug, PartialEq, Eq)]
struct Head {
    /// The owner of the repository that has the branch, for example the
    /// owner of a fork.
    owner: String,
    https_url: String,
    ssh_url: String,
    /// The name of the branch in that repository.
    branch: String,
}

impl Head {
    /// The repository that has the branch.
    fn key(&self) -> RepoKey {
        RepoKey::from_url(&self.https_url)
    }

    /// The local branches that can hold the head, in the order that
    /// `fork_branch` tries them.
    fn local_names(&self) -> [String; 2] {
        [
            self.branch.clone(),
            format!("{}-{}", self.owner, self.branch),
        ]
    }
}

/// The fields of a `Head`, in the order that `parse_head` reads. GitHub
/// gives no repository when the fork is deleted, and its fields are then
/// empty.
const HEAD_FIELDS: &str =
    "[.head.repo.owner.login, .head.repo.clone_url, .head.repo.ssh_url, .head.ref] | @tsv";

/// Ask GitHub for the head of a pull request. Git cannot give it:
/// `refs/pull/<number>/head` holds the commit, and not the fork. `gh` has
/// the login of the user, so a private fork is also possible.
fn head_of(key: &RepoKey, number: u64) -> Result<Head> {
    let text = gh(key, &format!("/pulls/{number}"), HEAD_FIELDS)
        .with_context(|| format!("gh cannot get pull request {number}"))?;
    parse_head(&text).with_context(|| {
        format!("GitHub has no fork for pull request {number}. the fork can be deleted")
    })
}

/// The repositories that `key` is a fork of: its parent and the source of
/// its network. The result is empty when `key` is not a fork.
fn forked_from(key: &RepoKey) -> Result<Vec<RepoKey>> {
    let text = gh(key, "", ".parent.clone_url, .source.clone_url")
        .with_context(|| format!("gh cannot get {key}"))?;
    // gh prints an empty line for a field that GitHub does not give.
    Ok(text
        .lines()
        .filter(|url| !url.is_empty())
        .map(RepoKey::from_url)
        .collect())
}

/// Ask GitHub for `endpoint` of the repository `key`, and give the output of
/// the filter `jq`.
fn gh(key: &RepoKey, endpoint: &str, jq: &str) -> Result<String> {
    let Some((host, path)) = key.host_and_path() else {
        bail!("{key} has no host");
    };
    let o = Command::new("gh")
        .args(["api", "--hostname", host])
        .arg(format!("repos/{path}{endpoint}"))
        .args(["--jq", jq])
        .output()
        .context("wt cannot run gh. wt asks gh for the fork of a pull request")?;
    if !o.status.success() {
        bail!("{}", String::from_utf8_lossy(&o.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// The head in the output of `HEAD_FIELDS`. The result is `None` when a
/// field is empty.
fn parse_head(text: &str) -> Option<Head> {
    let fields: Vec<&str> = text.trim_end_matches('\n').split('\t').collect();
    let &[owner, https_url, ssh_url, branch] = fields.as_slice() else {
        return None;
    };
    if fields.iter().any(|f| f.is_empty()) {
        return None;
    }
    Some(Head {
        owner: owner.to_string(),
        https_url: https_url.to_string(),
        ssh_url: ssh_url.to_string(),
        branch: branch.to_string(),
    })
}

/// The local branch for the head of a pull request from a fork. The head
/// can also be in origin or in its parent, when the pull request is in the
/// parent.
///
/// The function uses the remote that has the URL of the fork. If no remote
/// has it, the function adds a remote with the name of the owner.
///
/// The local branch has the name of the branch in the fork. When a local
/// branch has that name and tracks a different branch, for example `main`,
/// the name is `<owner>-<branch>`. The same head therefore always gives the
/// same local branch.
fn fork_branch(repo: &Repo, head: &Head) -> Result<Branch> {
    let remote = fork_remote(repo, head)?;
    let want = format!("refs/remotes/{remote}/{}", head.branch);
    let names = head.local_names();
    let Some(name) = names
        .iter()
        .find(|n| !has_local(repo, n) || repo.upstream(n).as_deref() == Some(want.as_str()))
    else {
        bail!(
            "the local branches {} and {} are present, and they track other branches",
            names[0],
            names[1]
        );
    };
    Ok(Branch {
        name: name.clone(),
        remote,
        upstream: head.branch.clone(),
    })
}

/// The remote for the fork of `head`. Add it if no remote has the URL of
/// the fork.
fn fork_remote(repo: &Repo, head: &Head) -> Result<String> {
    let key = head.key();
    let listing = git::out(&repo.common, &["remote", "-v"])?;
    // Each line is `<name>\t<url> (fetch)` or `<name>\t<url> (push)`.
    let remotes: Vec<&str> = listing
        .lines()
        .filter_map(|line| line.strip_suffix(" (fetch)")?.split_once('\t'))
        .filter(|(_, url)| RepoKey::from_url(url) == key)
        .map(|(name, _)| name)
        .collect();
    // More than one remote can have the fork: a person or an older wt can
    // add a second one. A local branch can track the head on one of them,
    // and a different remote gives a second branch for the same head.
    let tracked = remotes.iter().find(|r| {
        let want = format!("refs/remotes/{r}/{}", head.branch);
        head.local_names()
            .iter()
            .any(|n| repo.upstream(n).as_deref() == Some(want.as_str()))
    });
    if let Some(name) = tracked.or(remotes.first()) {
        return Ok(name.to_string());
    }

    let origin = git::out(&repo.common, &["remote", "get-url", "origin"])?;
    let url = fork_url(&origin, head);
    git::out(&repo.common, &["remote", "add", "--", &head.owner, url])?;
    eprintln!("  remote     {} ({url})", head.owner);
    Ok(head.owner.clone())
}

/// The URL of the fork in the form of the URL of origin. SSH stays SSH, so
/// a push to the fork uses the key that a push to origin uses.
fn fork_url<'a>(origin: &str, head: &'a Head) -> &'a str {
    if origin.starts_with("https://") || origin.starts_with("http://") {
        &head.https_url
    } else {
        &head.ssh_url
    }
}

/// The repository key and the number of a pull request, from its link.
///
///   https://github.com/owner/repo/pull/123        -> (github.com/owner/repo, 123)
///   https://github.com/owner/repo/pull/123/files  -> (github.com/owner/repo, 123)
fn pull_request(link: &str) -> Option<(RepoKey, u64)> {
    let (repo, rest) = link.split_once("/pull/")?;
    let number = rest.split(['/', '?', '#']).next()?.parse().ok()?;
    Some((RepoKey::from_url(repo), number))
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

/// Fetch the branch of `b` from its remote. A failure is a usual result:
/// the remote can have no such branch.
///
/// The user gives the name of the branch, and GitHub gives the name of the
/// branch of a fork. Both can start with '-'. Without `--`, git reads
/// `--upload-pack=<command>` as an option and runs the command.
fn fetch_branch(repo: &Repo, b: &Branch) {
    git::quiet(&repo.common, &["fetch", "--", &b.remote, &b.upstream]);
}

fn track_args(dest: &str, branch: &str, remote_ref: &str) -> Vec<String> {
    vec![
        "worktree".into(),
        "add".into(),
        "--track".into(),
        "-b".into(),
        branch.into(),
        "--".into(),
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
    use crate::testing::git;
    use std::path::Path;
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
                pull_request(link).map(|(key, number)| (key.to_string(), number)),
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

    /// A path would put the worktree away from the other worktrees, or give
    /// it the name of a different worktree. `wt cd` could not reach it.
    #[test]
    fn a_directory_name_is_one_component() {
        for name in ["review", "eng-1234", ".hidden"] {
            assert!(DirName::parse(name).is_ok(), "for {name}");
        }
        for path in [
            "",
            ".",
            "..",
            "../review",
            "/tmp/review",
            "nested/main",
            "review/",
            "./review",
        ] {
            assert!(DirName::parse(path).is_err(), "for {path}");
        }
    }

    /// A bare repository whose origin has one commit on master.
    fn repo() -> (TempDir, Repo) {
        let tmp = TempDir::new().unwrap();
        let origin = tmp.path().join("origin");
        let bare = tmp.path().join(".bare");
        git(
            tmp.path(),
            &["init", "-b", "master", origin.to_str().unwrap()],
        );
        git(&origin, &["commit", "--allow-empty", "-m", "one"]);
        git(tmp.path(), &["init", "--bare", bare.to_str().unwrap()]);
        git(
            &bare,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        git(&bare, &["fetch", "origin", "master"]);
        (tmp, Repo { common: bare })
    }

    /// A fork of origin that alice owns. Its `branch` has a commit that
    /// origin does not have.
    fn fork(tmp: &TempDir, branch: &str) -> Head {
        let fork = tmp.path().join("fork");
        git(
            tmp.path(),
            &[
                "clone",
                tmp.path().join("origin").to_str().unwrap(),
                fork.to_str().unwrap(),
            ],
        );
        git(&fork, &["switch", "-C", branch]);
        git(&fork, &["commit", "--allow-empty", "-m", "two"]);
        let url = fork.to_string_lossy().into_owned();
        Head {
            owner: "alice".into(),
            https_url: url.clone(),
            ssh_url: url,
            branch: branch.into(),
        }
    }

    fn commit(dir: &Path) -> String {
        crate::git::out(dir, &["rev-parse", "HEAD"]).unwrap()
    }

    /// A program that uses the library takes the worktree from the result.
    /// The second call finds the worktree that has the branch, and it does
    /// not make the directory that it names.
    #[test]
    fn the_result_is_the_worktree_that_has_the_branch() {
        let (tmp, repo) = repo();
        let other = DirName::parse("other").unwrap();

        let Outcome::Made(made) = run(&repo, "feature", None, Fetch::Never).unwrap() else {
            panic!("the first call must make the worktree");
        };
        let Outcome::Present(found) = run(&repo, "feature", Some(&other), Fetch::Never).unwrap()
        else {
            panic!("the second call must find the worktree");
        };

        let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        assert_eq!(real(&made), real(&tmp.path().join("feature")));
        assert_eq!(real(&found), real(&made));
        assert!(!tmp.path().join("other").exists());
    }

    /// A program that uses the library can pass a name that it did not
    /// write. Git reads `--upload-pack=<command>` as an option, and it runs
    /// the command when the remote is a path, as origin is here. Git reads
    /// `<src>:<dst>` as a refspec, and it writes `<dst>`.
    #[test]
    fn a_name_that_is_not_a_branch_name_reaches_no_git_command() {
        let (tmp, repo) = repo();
        let ran = tmp.path().join("ran");

        for name in [
            format!("--upload-pack=touch {}", ran.display()),
            "master:refs/heads/other".to_string(),
            "HEAD".to_string(),
        ] {
            let error = run(&repo, &name, None, Fetch::Lazy).unwrap_err();
            assert!(
                error.to_string().contains("is not a valid branch name"),
                "for {name}: {error:#}"
            );
        }
        assert!(!ran.exists());
        assert!(!has_local(&repo, "other"));
    }

    /// `git worktree add -b feature <path> origin/master` gives a branch that
    /// tracks master. The repository has no remote-tracking ref for the branch
    /// of the pull request yet.
    #[test]
    fn the_branch_of_a_pull_request_tracks_its_branch_on_origin() {
        let (tmp, repo) = repo();
        git(&tmp.path().join("origin"), &["branch", "feature"]);
        git(
            &repo.common,
            &["branch", "--track", "feature", "origin/master"],
        );

        track(&repo, &Branch::on_origin("feature")).unwrap();

        assert_eq!(
            repo.upstream("feature").as_deref(),
            Some("refs/remotes/origin/feature")
        );
        assert!(has_remote(&repo, "origin/feature"));
    }

    #[test]
    fn a_pull_request_from_a_fork_gives_a_worktree_that_tracks_the_fork() {
        let (tmp, repo) = repo();
        let head = fork(&tmp, "fix/keysets");

        let b = fork_branch(&repo, &head).unwrap();
        make(&repo, &b, true, None, Fetch::Lazy).unwrap();

        assert_eq!(
            repo.upstream("fix/keysets").as_deref(),
            Some("refs/remotes/alice/fix/keysets")
        );
        assert_eq!(
            commit(&tmp.path().join("fix-keysets")),
            commit(&tmp.path().join("fork"))
        );
    }

    /// A pull request from the master of a fork must not take the local
    /// master, which tracks origin/master.
    #[test]
    fn a_fork_branch_with_the_name_of_a_local_branch_gets_the_owner_in_front() {
        let (tmp, repo) = repo();
        git(
            &repo.common,
            &["branch", "--track", "master", "origin/master"],
        );
        let head = fork(&tmp, "master");

        let b = fork_branch(&repo, &head).unwrap();
        make(&repo, &b, true, None, Fetch::Lazy).unwrap();

        assert_eq!(
            repo.upstream("alice-master").as_deref(),
            Some("refs/remotes/alice/master")
        );
        assert_eq!(
            repo.upstream("master").as_deref(),
            Some("refs/remotes/origin/master")
        );
        // The next link to the same pull request finds the same branch, and
        // it adds no second remote.
        assert_eq!(fork_branch(&repo, &head).unwrap(), b);
    }

    /// GitHub ignores the case of a repository name, and a link has the case
    /// of the page. The link `.../Origin/pull/7` is to a pull request in
    /// origin, which is `.../origin`.
    #[test]
    fn a_link_to_origin_in_a_different_case_gives_the_branch_on_origin() {
        let (tmp, repo) = repo();
        let origin = tmp.path().join("origin");
        git(&origin, &["switch", "-c", "feature"]);
        git(&origin, &["commit", "--allow-empty", "-m", "two"]);
        git(&origin, &["update-ref", "refs/pull/7/head", "feature"]);
        let link = format!("https://{}/pull/7", tmp.path().join("Origin").display());

        assert_eq!(
            branch_of(&repo, &link, Fetch::Lazy).unwrap(),
            Branch::on_origin("feature")
        );
    }

    /// A remote can have the name of a fork in a case that GitHub does not
    /// give, for example `getfloresta/floresta` for `getfloresta/Floresta`.
    /// The fork is the same, so a second remote for it is not necessary.
    #[test]
    fn a_fork_that_a_remote_has_in_a_different_case_uses_that_remote() {
        let (_tmp, repo) = repo();
        git(
            &repo.common,
            &["remote", "add", "upstream", "git@github.com:alice/repo.git"],
        );
        let head = Head {
            owner: "alice".into(),
            https_url: "https://github.com/alice/Repo.git".into(),
            ssh_url: "git@github.com:alice/Repo.git".into(),
            branch: "feature".into(),
        };

        assert_eq!(fork_remote(&repo, &head).unwrap(), "upstream");
    }

    /// wt added the remote `alice`, and the branch of the head tracks it. A
    /// person or an older wt then added `aaa` for the same fork. Git lists
    /// `aaa` first.
    #[test]
    fn a_second_remote_for_a_fork_gives_the_same_branch() {
        let (tmp, repo) = repo();
        let head = fork(&tmp, "feature");
        let b = fork_branch(&repo, &head).unwrap();
        make(&repo, &b, true, None, Fetch::Lazy).unwrap();
        git(&repo.common, &["remote", "add", "aaa", &head.https_url]);

        assert_eq!(fork_branch(&repo, &head).unwrap(), b);
    }

    /// A new branch would hold none of the commits of the pull request.
    #[test]
    fn a_link_whose_branch_wt_cannot_fetch_makes_no_branch() {
        let (tmp, repo) = repo();
        let head = Head {
            branch: "absent".into(),
            ..fork(&tmp, "feature")
        };

        let b = fork_branch(&repo, &head).unwrap();

        assert!(make(&repo, &b, true, None, Fetch::Lazy).is_err());
        assert!(!has_local(&repo, "absent"));
        assert!(!tmp.path().join("absent").exists());
    }

    #[test]
    fn the_url_of_the_fork_has_the_form_of_the_url_of_origin() {
        let head = Head {
            owner: "alice".into(),
            https_url: "https://github.com/alice/repo.git".into(),
            ssh_url: "git@github.com:alice/repo.git".into(),
            branch: "feature".into(),
        };
        for (origin, url) in [
            (
                "https://github.com/owner/repo.git",
                "https://github.com/alice/repo.git",
            ),
            (
                "git@github.com:owner/repo.git",
                "git@github.com:alice/repo.git",
            ),
            (
                "ssh://git@github.com/owner/repo.git",
                "git@github.com:alice/repo.git",
            ),
        ] {
            assert_eq!(fork_url(origin, &head), url, "for {origin}");
        }
    }

    #[test]
    fn the_output_of_gh_gives_the_head() {
        assert_eq!(
            parse_head(
                "alice\thttps://github.com/alice/repo.git\tgit@github.com:alice/repo.git\tfix/x\n"
            ),
            Some(Head {
                owner: "alice".into(),
                https_url: "https://github.com/alice/repo.git".into(),
                ssh_url: "git@github.com:alice/repo.git".into(),
                branch: "fix/x".into(),
            })
        );
    }

    /// GitHub gives no repository for a pull request whose fork is deleted.
    #[test]
    fn a_deleted_fork_gives_no_head() {
        assert_eq!(parse_head("\t\t\tfix/x\n"), None);
    }
}
