use crate::git;
use anyhow::{Result, bail};
use std::path::Path;

/// Bring the worktree at `dir` to its upstream branch. You can run this
/// command many times.
///
/// The command fetches the upstream branch, and it moves the local branch when
/// the move cannot lose work:
///
/// - The branch is behind the upstream branch.
/// - The upstream branch has a different history, and the local branch is
///   where the upstream branch was before the fetch. The old commits are
///   therefore only the commits of the remote.
///
/// The command refuses a worktree with a change that is not committed, and a
/// branch with commits that the remote does not have. A worktree that is at the
/// upstream branch needs no move, so a change there does not stop the command.
/// An untracked file does not stop it either. Git refuses to overwrite one.
///
/// The progress goes to stderr. The function prints nothing on stdout.
pub fn run(dir: &Path) -> Result<()> {
    let Ok(head) = git::out(dir, &["symbolic-ref", "--quiet", "HEAD"]) else {
        bail!("the worktree has no branch checked out");
    };
    let branch = head.strip_prefix("refs/heads/").unwrap_or(&head);
    let config = |key: &str| git::out(dir, &["config", "--get", &format!("branch.{branch}.{key}")]);
    let (Ok(remote), Ok(merge)) = (config("remote"), config("merge")) else {
        bail!("{branch} has no upstream branch\n  run git branch --set-upstream-to to give it one");
    };

    // Git moves the remote-tracking ref in the fetch, and a rewrite then has no
    // other record of the old commits.
    let before = git::out(dir, &["rev-parse", "--verify", "--quiet", "@{upstream}"]).ok();
    eprintln!("  fetching   {remote} {merge}");
    git::out(dir, &["fetch", "--no-tags", "--", &remote, &merge])?;
    let tip = git::out(dir, &["rev-parse", "--verify", "FETCH_HEAD^{commit}"])?;
    let here = git::out(dir, &["rev-parse", "--verify", "HEAD"])?;
    if here == tip {
        eprintln!("  upstream   {branch} is at {merge}");
        return Ok(());
    }

    let changes = git::out(dir, &["status", "--porcelain", "--untracked-files=no"])?;
    if !changes.is_empty() {
        bail!("the worktree has changes that are not committed");
    }
    let behind = git::ok(dir, &["merge-base", "--is-ancestor", &here, &tip]);
    if !behind && before.as_deref() != Some(here.as_str()) {
        bail!("{branch} has commits that {remote} {merge} does not have");
    }
    git::out(dir, &["reset", "--quiet", "--keep", &tip])?;
    eprintln!("  moved      {branch} from {here} to {tip}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::git;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// A remote with the branch feat, and a clone in the .bare layout that has
    /// feat in a worktree. The branch tracks the branch of the remote.
    struct Fixture {
        _tmp: TempDir,
        remote: PathBuf,
        worktree: PathBuf,
    }

    impl Fixture {
        fn new() -> Fixture {
            let tmp = TempDir::new().unwrap();
            let remote = tmp.path().join("remote");
            let bare = tmp.path().join(".bare");
            let worktree = tmp.path().join("feat");
            git(
                tmp.path(),
                &["init", "-b", "feat", remote.to_str().unwrap()],
            );
            commit(&remote, "one");
            commit(&remote, "two");
            git(
                tmp.path(),
                &[
                    "clone",
                    "--bare",
                    remote.to_str().unwrap(),
                    bare.to_str().unwrap(),
                ],
            );
            git(
                &bare,
                &[
                    "config",
                    "remote.origin.fetch",
                    "+refs/heads/*:refs/remotes/origin/*",
                ],
            );
            git(&bare, &["fetch", "origin"]);
            git(
                &bare,
                &["worktree", "add", worktree.to_str().unwrap(), "feat"],
            );
            git(
                &worktree,
                &["branch", "--set-upstream-to=origin/feat", "feat"],
            );
            Fixture {
                _tmp: tmp,
                remote,
                worktree,
            }
        }

        /// The author adds a commit. The result is the new tip.
        fn pushes(&self, name: &str) -> String {
            commit(&self.remote, name);
            tip(&self.remote)
        }

        /// The author replaces the last commit. The result is the new tip.
        fn rewrites(&self, name: &str) -> String {
            git(&self.remote, &["reset", "--hard", "HEAD~1"]);
            self.pushes(name)
        }

        fn follow(&self) -> Result<()> {
            run(&self.worktree)
        }

        fn here(&self) -> String {
            tip(&self.worktree)
        }

        fn read(&self, name: &str) -> String {
            fs::read_to_string(self.worktree.join(name)).unwrap()
        }

        fn write(&self, name: &str) {
            fs::write(self.worktree.join(name), "mine").unwrap();
        }
    }

    fn commit(dir: &Path, name: &str) {
        fs::write(dir.join(name), name).unwrap();
        git(dir, &["add", name]);
        git(dir, &["commit", "-m", name]);
    }

    fn tip(dir: &Path) -> String {
        crate::git::out(dir, &["rev-parse", "HEAD"]).unwrap()
    }

    fn says(result: Result<()>, words: &str) {
        let why = format!("{:#}", result.unwrap_err());
        assert!(why.contains(words), "{why}");
    }

    #[test]
    fn a_branch_at_the_upstream_branch_stays() {
        let f = Fixture::new();
        let here = f.here();
        f.follow().unwrap();
        assert_eq!(f.here(), here);
    }

    /// No move is necessary, so the change is safe.
    #[test]
    fn a_branch_at_the_upstream_branch_with_a_change_stays() {
        let f = Fixture::new();
        let here = f.here();
        f.write("two");
        f.follow().unwrap();
        assert_eq!(f.here(), here);
        assert_eq!(f.read("two"), "mine");
    }

    #[test]
    fn a_branch_behind_the_upstream_branch_moves() {
        let f = Fixture::new();
        let tip = f.pushes("three");
        f.follow().unwrap();
        assert_eq!(f.here(), tip);
        assert_eq!(f.read("three"), "three");
    }

    #[test]
    fn a_rewrite_moves_a_branch_that_has_only_the_commits_of_the_remote() {
        let f = Fixture::new();
        let tip = f.rewrites("rewritten");
        f.follow().unwrap();
        assert_eq!(f.here(), tip);
        assert_eq!(f.read("rewritten"), "rewritten");
        assert!(!f.worktree.join("two").exists());
    }

    /// The fetch moves the remote-tracking ref to the new tip, so the next
    /// rewrite also finds the branch where the upstream branch was.
    #[test]
    fn each_rewrite_moves_the_branch() {
        let f = Fixture::new();
        f.pushes("three");
        f.follow().unwrap();
        for name in ["first", "second"] {
            let tip = f.rewrites(name);
            f.follow().unwrap();
            assert_eq!(f.here(), tip);
        }
    }

    #[test]
    fn a_change_stops_a_move() {
        for staged in [false, true] {
            let f = Fixture::new();
            let here = f.here();
            f.pushes("three");
            f.write("two");
            if staged {
                git(&f.worktree, &["add", "two"]);
            }
            says(f.follow(), "changes that are not committed");
            assert_eq!(f.here(), here);
            assert_eq!(f.read("two"), "mine");
        }
    }

    #[test]
    fn commits_that_the_remote_does_not_have_stop_a_move() {
        let ahead = Fixture::new();
        commit(&ahead.worktree, "mine");
        let here = ahead.here();
        says(ahead.follow(), "does not have");
        assert_eq!(ahead.here(), here);

        let diverged = Fixture::new();
        commit(&diverged.worktree, "mine");
        let here = diverged.here();
        diverged.rewrites("rewritten");
        says(diverged.follow(), "does not have");
        assert_eq!(diverged.here(), here);
    }

    /// The links that wt makes are untracked files.
    #[test]
    fn an_untracked_file_does_not_stop_a_move() {
        let f = Fixture::new();
        f.write("linked");
        let tip = f.pushes("three");
        f.follow().unwrap();
        assert_eq!(f.here(), tip);
        assert_eq!(f.read("linked"), "mine");
    }

    #[test]
    fn an_untracked_file_that_the_upstream_branch_has_stops_a_move() {
        let f = Fixture::new();
        let here = f.here();
        f.write("three");
        f.pushes("three");
        says(f.follow(), "'three'");
        assert_eq!(f.here(), here);
        assert_eq!(f.read("three"), "mine");
    }

    #[test]
    fn a_failed_fetch_moves_nothing() {
        let f = Fixture::new();
        let here = f.here();
        git(&f.remote, &["branch", "-m", "feat", "other"]);
        says(f.follow(), "refs/heads/feat");
        assert_eq!(f.here(), here);
    }

    #[test]
    fn a_worktree_with_no_branch_moves_nothing() {
        let f = Fixture::new();
        f.pushes("three");
        git(&f.worktree, &["switch", "--detach"]);
        let here = f.here();
        says(f.follow(), "no branch checked out");
        assert_eq!(f.here(), here);
    }

    #[test]
    fn a_branch_with_no_upstream_branch_moves_nothing() {
        let f = Fixture::new();
        f.pushes("three");
        git(&f.worktree, &["branch", "--unset-upstream"]);
        let here = f.here();
        says(f.follow(), "feat has no upstream branch");
        assert_eq!(f.here(), here);
    }

    /// `gh pr checkout` leaves this for a pull request from a fork. No
    /// remote-tracking ref records the old commits, so only a fast-forward can
    /// be safe.
    #[test]
    fn a_branch_that_merges_from_a_pull_request_ref_fast_forwards_and_refuses_a_rewrite() {
        let f = Fixture::new();
        git(&f.remote, &["update-ref", "refs/pull/7/head", "feat"]);
        git(
            &f.worktree,
            &["config", "branch.feat.merge", "refs/pull/7/head"],
        );
        let tip = f.pushes("three");
        git(&f.remote, &["update-ref", "refs/pull/7/head", "feat"]);
        f.follow().unwrap();
        assert_eq!(f.here(), tip);

        f.rewrites("rewritten");
        git(&f.remote, &["update-ref", "refs/pull/7/head", "feat"]);
        says(f.follow(), "does not have");
        assert_eq!(f.here(), tip);
    }
}
