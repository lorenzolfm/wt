use crate::git;
use anyhow::{Context, Result, bail};
use std::path::Path;

/// Bring the worktree at `dir` to its upstream branch. You can run this
/// command many times. A refusal keeps the record of a rewrite, so the next
/// run can move the branch.
///
/// The command fetches the upstream branch, and it moves the local branch when
/// the move cannot lose work:
///
/// - The branch is behind the upstream branch.
/// - The upstream branch has a different history, and the local branch is at
///   the remote-tracking ref. The old commits are therefore only the commits
///   of the remote. A different fetch moves that ref, and the command then
///   refuses the move, because it cannot tell them from new work.
///
/// The command refuses a worktree with a change that is not committed or with
/// a git operation that is not finished, and a branch with commits that the
/// remote does not have. A worktree that is at the upstream branch needs no
/// move, so a change there does not stop the command.
///
/// The command also refuses to overwrite an untracked file or an ignored file.
/// A file at a different path does not stop it, and that file stays.
///
/// The progress goes to stderr. The function prints nothing on stdout.
pub fn run(dir: &Path) -> Result<()> {
    let inside = git::out(dir, &["rev-parse", "--is-inside-work-tree"])
        .with_context(|| format!("there is no git repository at {}", dir.display()))?;
    if inside != "true" {
        bail!("{} is not in a worktree", dir.display());
    }
    let Ok(head) = git::out(dir, &["symbolic-ref", "--quiet", "HEAD"]) else {
        bail!("the worktree has no branch checked out");
    };
    let branch = head.strip_prefix("refs/heads/").unwrap_or(&head);
    let config = |key: &str| git::out(dir, &["config", "--get", &format!("branch.{branch}.{key}")]);
    let (Ok(remote), Ok(merge)) = (config("remote"), config("merge")) else {
        bail!("{branch} has no upstream branch\n  run git branch --set-upstream-to to give it one");
    };

    // The remote-tracking ref is the only record of the old commits of a
    // rewrite. With an empty refmap the fetch does not move it, so a refusal
    // keeps the record for the next run.
    let before = git::out(dir, &["rev-parse", "--verify", "--quiet", "@{upstream}"]).ok();
    eprintln!("  fetching   {remote} {merge}");
    git::out(
        dir,
        &["fetch", "--no-tags", "--refmap=", "--", &remote, &merge],
    )?;
    let tip = git::out(dir, &["rev-parse", "--verify", "FETCH_HEAD^{commit}"])?;
    let here = git::out(dir, &["rev-parse", "--verify", "HEAD"])?;
    if here == tip {
        move_tracking_ref(dir, &head, &tip)?;
        eprintln!("  upstream   {branch} is at {merge}");
        return Ok(());
    }

    let changes = git::out(dir, &["status", "--porcelain", "--untracked-files=no"])?;
    if !changes.is_empty() {
        bail!("the worktree has changes that are not committed");
    }
    let behind = git::ok(dir, &["merge-base", "--is-ancestor", &here, &tip]);
    if !behind && before.as_deref() != Some(here.as_str()) {
        bail!(
            "{branch} has commits that {remote} {merge} does not have\n  \
             wt cannot tell your work from the old commits of a rewrite\n  \
             if they are not your work, run git switch --no-overwrite-ignore -C {branch} {tip}"
        );
    }
    // Not `git reset --keep`: it overwrites an ignored file, and each link that
    // wt makes is an ignored file. It also moves the branch under a paused `git
    // am`, and it ends a paused cherry-pick.
    git::out(
        dir,
        &[
            "switch",
            "--quiet",
            "--no-overwrite-ignore",
            "-C",
            branch,
            &tip,
        ],
    )?;
    move_tracking_ref(dir, &head, &tip)?;
    eprintln!("  moved      {branch} from {here} to {tip}");
    Ok(())
}

/// Move the remote-tracking ref of the branch `head` to `tip`, so the next
/// rewrite finds the branch at that ref. A branch that merges from
/// `refs/pull/N/head` has no such ref, and a local upstream branch is not one.
/// Neither changes.
///
/// The function reads `%(upstream)` and not `@{upstream}`, which fails when the
/// ref is absent. `wt prune` reads an upstream branch with no ref as a branch
/// that the remote deleted, so the ref must come back.
fn move_tracking_ref(dir: &Path, head: &str, tip: &str) -> Result<()> {
    let upstream = git::out(dir, &["for-each-ref", "--format=%(upstream)", head])?;
    if upstream.starts_with("refs/remotes/") {
        git::out(dir, &["update-ref", &upstream, tip])?;
    }
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
        bare: PathBuf,
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
                bare,
                worktree,
            }
        }

        /// The clone ignores `pattern`. The remote does not, so the author can
        /// add a path that the clone ignores.
        fn ignores(&self, pattern: &str) {
            let info = self.bare.join("info");
            fs::create_dir_all(&info).unwrap();
            fs::write(info.join("exclude"), pattern).unwrap();
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
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, name).unwrap();
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

    /// A move also moves the remote-tracking ref to the new tip, so the next
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

    /// A refusal keeps the record of the old commits, so the next run can move
    /// the branch.
    #[test]
    fn a_rewrite_moves_the_branch_after_a_refusal() {
        let change = Fixture::new();
        change.write("two");
        let tip = change.rewrites("rewritten");
        says(change.follow(), "changes that are not committed");
        git(&change.worktree, &["checkout", "--", "two"]);
        change.follow().unwrap();
        assert_eq!(change.here(), tip);

        let untracked = Fixture::new();
        untracked.write("rewritten");
        let tip = untracked.rewrites("rewritten");
        says(untracked.follow(), "rewritten");
        fs::remove_file(untracked.worktree.join("rewritten")).unwrap();
        untracked.follow().unwrap();
        assert_eq!(untracked.here(), tip);
    }

    /// A different fetch moves the remote-tracking ref, and then nothing
    /// records the old commits. The message gives the command for a move.
    #[test]
    fn a_rewrite_that_a_different_fetch_saw_first_stops_a_move() {
        let f = Fixture::new();
        let here = f.here();
        let tip = f.rewrites("rewritten");
        git(&f.bare, &["fetch", "origin"]);
        let command = format!("git switch --no-overwrite-ignore -C feat {tip}");
        says(f.follow(), &command);
        assert_eq!(f.here(), here);
    }

    /// `wt prune` reads an upstream branch with no remote-tracking ref as a
    /// branch that the remote deleted.
    #[test]
    fn a_branch_at_the_upstream_branch_gets_its_remote_tracking_ref() {
        let f = Fixture::new();
        let here = f.here();
        git(&f.bare, &["update-ref", "-d", "refs/remotes/origin/feat"]);
        f.follow().unwrap();
        let tracking = crate::git::out(&f.bare, &["rev-parse", "refs/remotes/origin/feat"]);
        assert_eq!(tracking.unwrap(), here);
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

    #[test]
    fn an_untracked_file_does_not_stop_a_move() {
        for ignored in [false, true] {
            let f = Fixture::new();
            if ignored {
                f.ignores("mine");
            }
            f.write("mine");
            let tip = f.pushes("three");
            f.follow().unwrap();
            assert_eq!(f.here(), tip);
            assert_eq!(f.read("mine"), "mine");
        }
    }

    #[test]
    fn an_untracked_file_that_the_upstream_branch_has_stops_a_move() {
        for ignored in [false, true] {
            let f = Fixture::new();
            if ignored {
                f.ignores("three");
            }
            let here = f.here();
            f.write("three");
            f.pushes("three");
            says(f.follow(), "three");
            assert_eq!(f.here(), here);
            assert_eq!(f.read("three"), "mine");
        }
    }

    #[test]
    fn an_ignored_directory_that_the_upstream_branch_replaces_stops_a_move() {
        let f = Fixture::new();
        f.ignores("build/");
        let here = f.here();
        fs::create_dir(f.worktree.join("build")).unwrap();
        f.write("build/output");
        f.pushes("build");
        says(f.follow(), "build");
        assert_eq!(f.here(), here);
        assert_eq!(f.read("build/output"), "mine");
    }

    /// The links that wt makes are ignored files. The upstream branch can have
    /// the path of a link or a path below it.
    #[test]
    fn a_link_that_the_upstream_branch_replaces_stops_a_move() {
        for tracked in ["linked", "linked/three"] {
            let f = Fixture::new();
            f.ignores("linked");
            let store = f.bare.join("shared");
            fs::create_dir(&store).unwrap();
            std::os::unix::fs::symlink(&store, f.worktree.join("linked")).unwrap();
            let here = f.here();
            f.pushes(tracked);
            says(f.follow(), "linked");
            assert_eq!(f.here(), here);
            assert_eq!(fs::read_link(f.worktree.join("linked")).unwrap(), store);
            assert_eq!(fs::read_dir(&store).unwrap().count(), 0);
        }
    }

    /// A cherry-pick that stops with no change leaves nothing for the status
    /// to show.
    #[test]
    fn a_paused_cherry_pick_stops_a_move() {
        let f = Fixture::new();
        let here = f.here();
        assert!(!crate::git::ok(&f.worktree, &["cherry-pick", &here]));
        f.pushes("three");
        says(f.follow(), "cherry-pick");
        assert_eq!(f.here(), here);
    }

    #[test]
    fn a_directory_that_is_not_in_a_worktree_is_refused() {
        let f = Fixture::new();
        f.pushes("three");
        says(run(&f.bare), "is not in a worktree");
        says(
            run(&f.worktree.join("absent")),
            "there is no git repository",
        );
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
