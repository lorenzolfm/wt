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
    let Tracking {
        head,
        branch,
        remote,
        merge,
    } = match Head::of(dir)? {
        Head::Detached => bail!("the worktree has no branch checked out"),
        Head::Untracked { branch } => bail!(
            "{branch} has no upstream branch\n  run git branch --set-upstream-to to give it one"
        ),
        Head::Tracking(t) => t,
    };
    let branch = branch.as_str();

    let Fetched { before, tip } = fetch(dir, &remote, &merge)?;
    let here = git::out(dir, &["rev-parse", "--verify", "HEAD"])?;
    let plan = Plan::of(dir, &here, before.as_deref(), &tip);
    if plan == Plan::Stays {
        move_tracking_ref(dir, &head, &tip)?;
        eprintln!("  upstream   {branch} is at {merge}");
        return Ok(());
    }

    let changes = git::out(dir, &["status", "--porcelain", "--untracked-files=no"])?;
    if !changes.is_empty() {
        bail!("the worktree has changes that are not committed");
    }
    if plan == Plan::Refuses {
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

/// Fetch the upstream branch of the worktree at `dir`, and tell what it has
/// that the local branch does not have, and what `wt follow` does with it. The
/// command changes no branch and no file, so a change in the worktree does not
/// stop it.
///
/// The fetch moves the remote-tracking ref only when the new tip contains the
/// old one. After a rewrite the ref is the only record of the old commits, and
/// `run` needs that record to move the branch.
///
/// A failure is a warning, and the function changed nothing. The report and
/// the warning go to stderr. The function prints nothing on stdout.
pub fn check(dir: &Path) {
    match inspect(dir) {
        Ok(news) => eprintln!("  upstream   {news}"),
        Err(e) => eprintln!("warning: wt cannot tell what is new upstream: {e:#}"),
    }
}

/// The branch of a worktree, and its upstream branch from the git config.
enum Head {
    /// The worktree is on no branch.
    Detached,
    /// The branch has no upstream branch.
    Untracked {
        branch: String,
    },
    Tracking(Tracking),
}

struct Tracking {
    /// For example `refs/heads/feat`.
    head: String,
    /// For example `feat`.
    branch: String,
    remote: String,
    /// The branch on the remote, for example `refs/heads/feat`.
    merge: String,
}

impl Head {
    fn of(dir: &Path) -> Result<Head> {
        let Ok(head) = git::out(dir, &["symbolic-ref", "--quiet", "HEAD"]) else {
            return Ok(Head::Detached);
        };
        let Some(branch) = head.strip_prefix("refs/heads/") else {
            bail!("HEAD is at {head}, and not at a branch");
        };
        let branch = branch.to_string();
        let config =
            |key: &str| git::out(dir, &["config", "--get", &format!("branch.{branch}.{key}")]);
        let (Ok(remote), Ok(merge)) = (config("remote"), config("merge")) else {
            return Ok(Head::Untracked { branch });
        };
        Ok(Head::Tracking(Tracking {
            head,
            branch,
            remote,
            merge,
        }))
    }
}

/// The result of a fetch of the upstream branch.
struct Fetched {
    /// The remote-tracking ref before the fetch. It is absent for a branch
    /// with no such ref, and for a ref that is not there.
    before: Option<String>,
    /// The commit that the remote has now.
    tip: String,
}

/// Fetch `merge` from `remote`. The fetch moves no ref.
///
/// The remote-tracking ref is the only record of the old commits of a
/// rewrite. With an empty refmap the fetch does not move it, so a refusal
/// keeps the record for the next run.
fn fetch(dir: &Path, remote: &str, merge: &str) -> Result<Fetched> {
    let before = git::out(dir, &["rev-parse", "--verify", "--quiet", "@{upstream}"]).ok();
    eprintln!("  fetching   {remote} {merge}");
    git::out(
        dir,
        &["fetch", "--no-tags", "--refmap=", "--", remote, merge],
    )?;
    let tip = git::out(dir, &["rev-parse", "--verify", "FETCH_HEAD^{commit}"])?;
    Ok(Fetched { before, tip })
}

/// What `wt follow` does with the branch. `run` acts on it, and `check`
/// tells it, so the report and the command cannot disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Plan {
    /// The branch is at the upstream branch.
    Stays,
    /// The branch is behind the upstream branch.
    FastForward,
    /// The upstream branch has a different history, and the branch is at
    /// the remote-tracking ref. The old commits are therefore only the
    /// commits of the remote.
    FollowsRewrite,
    /// The branch has commits that the upstream branch does not have, and
    /// wt cannot tell them from the old commits of a rewrite.
    Refuses,
}

impl Plan {
    /// The plan for the branch at `here`, with the remote-tracking ref at
    /// `before` and the upstream branch at `tip`.
    fn of(dir: &Path, here: &str, before: Option<&str>, tip: &str) -> Plan {
        if here == tip {
            Plan::Stays
        } else if git::ok(dir, &["merge-base", "--is-ancestor", here, tip]) {
            Plan::FastForward
        } else if before == Some(here) {
            Plan::FollowsRewrite
        } else {
            Plan::Refuses
        }
    }
}

/// What the upstream branch has, from the view of the local branch.
#[derive(Debug, PartialEq, Eq)]
enum News {
    /// The worktree is on no branch.
    NoBranch,
    /// The branch has no upstream branch.
    NoUpstream { branch: String },
    Compared {
        branch: String,
        /// For example `origin/feat`.
        upstream: String,
        /// The commits that only the local branch has.
        mine: usize,
        /// The commits that only the upstream branch has.
        theirs: usize,
        plan: Plan,
    },
}

impl std::fmt::Display for News {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let commits = |n: usize| match n {
            1 => "1 commit".to_string(),
            n => format!("{n} commits"),
        };
        match self {
            News::NoBranch => write!(f, "the worktree has no branch checked out"),
            News::NoUpstream { branch } => write!(f, "{branch} has no upstream branch"),
            News::Compared {
                branch,
                upstream,
                mine,
                theirs,
                plan,
            } => match plan {
                Plan::Stays => write!(f, "{branch} is at {upstream}"),
                Plan::FastForward => write!(
                    f,
                    "{upstream} has {} that {branch} does not have\n  \
                     run wt follow to bring them here",
                    commits(*theirs)
                ),
                Plan::FollowsRewrite if *theirs == 0 => write!(
                    f,
                    "the author removed {} from {upstream}\n  \
                     run wt follow to remove them here",
                    commits(*mine)
                ),
                Plan::FollowsRewrite => write!(
                    f,
                    "{upstream} has a different history, with {} in place of {}\n  \
                     run wt follow to bring it here",
                    commits(*theirs),
                    commits(*mine)
                ),
                Plan::Refuses if *theirs == 0 => write!(
                    f,
                    "{branch} has {} that {upstream} does not have",
                    commits(*mine)
                ),
                Plan::Refuses => write!(
                    f,
                    "{branch} and {upstream} have diverged: {} here, {} there",
                    commits(*mine),
                    commits(*theirs)
                ),
            },
        }
    }
}

/// Fetch the upstream branch and compare it with the local branch.
fn inspect(dir: &Path) -> Result<News> {
    let Tracking {
        head,
        branch,
        remote,
        merge,
    } = match Head::of(dir)? {
        Head::Detached => return Ok(News::NoBranch),
        Head::Untracked { branch } => return Ok(News::NoUpstream { branch }),
        Head::Tracking(t) => t,
    };
    let upstream = match merge.strip_prefix("refs/heads/") {
        Some(name) => format!("{remote}/{name}"),
        None => format!("{remote} {merge}"),
    };

    let Fetched { before, tip } = fetch(dir, &remote, &merge)?;
    let here = git::out(dir, &["rev-parse", "--verify", "HEAD"])?;
    let plan = Plan::of(dir, &here, before.as_deref(), &tip);
    advance_tracking_ref(dir, &head, &tip)?;

    let counts = git::out(
        dir,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("{here}...{tip}"),
        ],
    )?;
    let Some((mine, theirs)) = counts.split_once('\t') else {
        bail!("git rev-list gave {counts}, and not two counts");
    };
    Ok(News::Compared {
        branch,
        upstream,
        mine: mine.parse()?,
        theirs: theirs.parse()?,
        plan,
    })
}

/// Move the remote-tracking ref of the branch `head` to `tip` when `tip`
/// contains the ref, as a usual fetch does. The ref stays when `tip` does not
/// contain it, and then it keeps the record of a rewrite for `run`.
///
/// An absent ref holds no record, so the function makes it. A branch with no
/// remote-tracking ref changes nothing.
fn advance_tracking_ref(dir: &Path, head: &str, tip: &str) -> Result<()> {
    let upstream = git::out(dir, &["for-each-ref", "--format=%(upstream)", head])?;
    if !upstream.starts_with("refs/remotes/") {
        return Ok(());
    }
    let old = git::out(dir, &["rev-parse", "--verify", "--quiet", &upstream]).ok();
    if let Some(old) = &old
        && !git::ok(dir, &["merge-base", "--is-ancestor", old, tip])
    {
        return Ok(());
    }
    // With `old`, git keeps a ref that a different fetch changed. For git an
    // empty old value means that the ref must be absent.
    git::out(
        dir,
        &["update-ref", &upstream, tip, old.as_deref().unwrap_or("")],
    )?;
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

    fn tracking(f: &Fixture) -> String {
        crate::git::out(&f.bare, &["rev-parse", "refs/remotes/origin/feat"]).unwrap()
    }

    fn compared(mine: usize, theirs: usize, plan: Plan) -> News {
        News::Compared {
            branch: "feat".into(),
            upstream: "origin/feat".into(),
            mine,
            theirs,
            plan,
        }
    }

    #[test]
    fn a_check_of_a_branch_at_the_upstream_branch_finds_nothing_new() {
        let f = Fixture::new();
        assert_eq!(inspect(&f.worktree).unwrap(), compared(0, 0, Plan::Stays));
    }

    /// The check moves the remote-tracking ref as a usual fetch does, and
    /// changes no branch and no file.
    #[test]
    fn a_check_counts_the_new_commits_and_moves_nothing_but_the_ref() {
        let f = Fixture::new();
        let here = f.here();
        f.write("two");
        f.pushes("three");
        let tip = f.pushes("four");
        assert_eq!(
            inspect(&f.worktree).unwrap(),
            compared(0, 2, Plan::FastForward)
        );
        assert_eq!(f.here(), here);
        assert_eq!(f.read("two"), "mine");
        assert_eq!(tracking(&f), tip);
    }

    #[test]
    fn a_check_counts_the_commits_on_each_side() {
        let f = Fixture::new();
        commit(&f.worktree, "mine");
        assert_eq!(inspect(&f.worktree).unwrap(), compared(1, 0, Plan::Refuses));
        f.pushes("three");
        assert_eq!(inspect(&f.worktree).unwrap(), compared(1, 1, Plan::Refuses));
    }

    /// The ref is the record of the old commits, so the check keeps it, and
    /// wt follow can still move the branch after the check.
    #[test]
    fn a_check_of_a_rewrite_keeps_the_record_for_wt_follow() {
        let f = Fixture::new();
        let before = tracking(&f);
        let tip = f.rewrites("rewritten");
        assert_eq!(
            inspect(&f.worktree).unwrap(),
            compared(1, 1, Plan::FollowsRewrite)
        );
        assert_eq!(tracking(&f), before);
        f.follow().unwrap();
        assert_eq!(f.here(), tip);
    }

    /// The author removes the last commit. The commit is of the remote, not
    /// of the user, so the check names wt follow, and wt follow removes it.
    #[test]
    fn a_check_of_a_removed_commit_names_wt_follow() {
        let f = Fixture::new();
        git(&f.remote, &["reset", "--hard", "HEAD~1"]);
        let tip = tip(&f.remote);
        let news = inspect(&f.worktree).unwrap();
        assert_eq!(news, compared(1, 0, Plan::FollowsRewrite));
        assert!(news.to_string().contains("wt follow"), "{news}");
        f.follow().unwrap();
        assert_eq!(f.here(), tip);
    }

    /// A check after a different fetch finds no record, as wt follow does.
    #[test]
    fn a_check_of_a_rewrite_that_a_different_fetch_saw_first_names_no_move() {
        let f = Fixture::new();
        f.rewrites("rewritten");
        git(&f.bare, &["fetch", "origin"]);
        assert_eq!(inspect(&f.worktree).unwrap(), compared(1, 1, Plan::Refuses));
        says(f.follow(), "does not have");
    }

    #[test]
    fn a_check_makes_an_absent_ref() {
        let f = Fixture::new();
        let here = f.here();
        git(&f.bare, &["update-ref", "-d", "refs/remotes/origin/feat"]);
        assert_eq!(inspect(&f.worktree).unwrap(), compared(0, 0, Plan::Stays));
        assert_eq!(tracking(&f), here);
    }

    #[test]
    fn a_check_without_an_upstream_branch_uses_no_network() {
        let f = Fixture::new();
        git(&f.worktree, &["branch", "--unset-upstream"]);
        let news = inspect(&f.worktree).unwrap();
        assert_eq!(
            news,
            News::NoUpstream {
                branch: "feat".into()
            }
        );
        git(&f.worktree, &["switch", "--detach"]);
        assert_eq!(inspect(&f.worktree).unwrap(), News::NoBranch);
    }

    #[test]
    fn a_failed_check_changes_nothing() {
        let f = Fixture::new();
        let before = tracking(&f);
        git(&f.remote, &["branch", "-m", "feat", "other"]);
        says(inspect(&f.worktree).map(drop), "refs/heads/feat");
        assert_eq!(tracking(&f), before);
    }

    #[test]
    fn the_report_names_wt_follow_when_wt_follow_moves_the_branch() {
        let report = |plan| compared(1, 1, plan).to_string();
        assert!(report(Plan::FastForward).contains("wt follow"));
        assert!(report(Plan::FollowsRewrite).contains("wt follow"));
        assert!(!report(Plan::Refuses).contains("wt follow"));
        assert!(!report(Plan::Stays).contains("wt follow"));
        let one = compared(0, 1, Plan::FastForward).to_string();
        assert!(one.contains("1 commit that"), "{one}");
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
