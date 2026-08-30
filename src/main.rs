mod cmd;
mod config;
mod git;
mod hook;
mod repo;
mod seed;

use anyhow::Result;
use clap::{Parser, Subcommand};
use repo::Repo;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "wt",
    about = "Git worktrees, plus the files that git ignores",
    after_help = "wt <worktree> is the short form of wt cd <worktree>.",
    version
)]
struct Cli {
    /// Use this repository. The default is the repository of the current directory
    #[arg(long, global = true, value_name = "PATH")]
    repo: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Make the store, the config entry and the hook
    Init,

    /// Move ignored paths into the store and make the links
    #[command(visible_alias = "sh")]
    Share {
        /// The paths. Each path is relative to the current worktree
        #[arg(required = true)]
        paths: Vec<String>,
        /// Replace a file that is different. The default is to keep it
        #[arg(long)]
        force: bool,
    },

    /// Print each worktree, its branch and the condition of its links
    #[command(visible_alias = "l")]
    Ls,

    /// Change to a worktree, by directory name or by branch name
    ///
    /// wt <worktree> is the short form. A command wins over a worktree with
    /// the same name, so a worktree named ls needs the long form wt cd ls.
    #[command(visible_alias = "c")]
    Cd { worktree: String },

    /// Compare each worktree with the config and make the links
    #[command(visible_alias = "s")]
    Sync {
        /// Replace a file that is different. The default is to keep it
        #[arg(long)]
        force: bool,
    },

    /// Make a worktree for a branch
    #[command(visible_alias = "a")]
    Add {
        branch: String,
        /// The directory name. The default is the branch name with '-' for '/'
        dir: Option<String>,
        /// Fetch first, also if the branch is known
        #[arg(long, conflicts_with = "no_fetch")]
        fetch: bool,
        /// Do not use the network
        #[arg(long)]
        no_fetch: bool,
    },

    /// Remove each worktree whose work is finished
    #[command(visible_alias = "p")]
    Prune {
        /// Print the worktrees that the command would remove. Change nothing
        #[arg(long)]
        dry_run: bool,
        /// Remove a worktree also if it has a modified or untracked file
        #[arg(long)]
        force: bool,
        /// Do not use the network. Ask origin nothing
        #[arg(long)]
        no_fetch: bool,
    },

    /// Remove a worktree and delete its branch
    #[command(visible_alias = "d")]
    Delete {
        worktree: String,
        /// Remove the worktree also if it has a modified or untracked file
        #[arg(long)]
        force: bool,
    },

    /// Clone a repository into the .bare layout
    Clone { url: String, dir: Option<String> },

    /// Print the shell integration
    ShellInit {
        #[arg(default_value = "fish")]
        shell: String,
    },

    #[command(hide = true, name = "__branches")]
    Branches,

    #[command(hide = true, name = "__worktrees")]
    Worktrees,

    #[command(hide = true, name = "__shareable")]
    Shareable,

    /// `wt <worktree>` is the short form of `wt cd <worktree>`
    #[command(external_subcommand)]
    Bare(Vec<String>),
}

fn main() -> ExitCode {
    // Git runs the hook through a link with the name post-checkout. The
    // current directory is the worktree. The program reads its own name to
    // find this condition.
    let argv: Vec<String> = std::env::args().collect();
    let invoked_as = argv
        .first()
        .map(|a| {
            Path::new(a)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_default();
    if invoked_as == "post-checkout" {
        return ExitCode::from(hook::run_as_hook(&argv[1..]) as u8);
    }

    match dispatch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("wt: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch() -> Result<()> {
    let cli = Cli::parse();
    let at = cli.repo.as_deref();

    match cli.command {
        Command::ShellInit { shell } => cmd::shell_init::run(&shell),
        Command::Clone { url, dir } => cmd::clone::run(&url, dir.as_deref()),
        other => {
            let repo = Repo::discover(at)?;
            match other {
                Command::Init => cmd::init::run(&repo),
                Command::Share { paths, force } => cmd::share::run(&repo, &paths, force),
                Command::Ls => cmd::ls::run(&repo),
                Command::Cd { worktree } => cmd::cd::run(&repo, &worktree),
                Command::Sync { force } => cmd::sync::run(&repo, force),
                Command::Add {
                    branch,
                    dir,
                    fetch,
                    no_fetch,
                } => {
                    let mode = if no_fetch {
                        cmd::add::Fetch::Never
                    } else if fetch {
                        cmd::add::Fetch::Always
                    } else {
                        cmd::add::Fetch::Lazy
                    };
                    cmd::add::run(&repo, &branch, dir.as_deref(), mode)
                }
                Command::Prune {
                    dry_run,
                    force,
                    no_fetch,
                } => cmd::prune::run(&repo, dry_run, force, !no_fetch),
                Command::Delete { worktree, force } => cmd::delete::run(&repo, &worktree, force),
                Command::Branches => cmd::completions::branches(&repo),
                Command::Worktrees => cmd::completions::worktrees(&repo),
                Command::Shareable => cmd::completions::shareable(&repo),
                Command::Bare(words) => cmd::cd::bare(&repo, &words),
                Command::ShellInit { .. } | Command::Clone { .. } => unreachable!(),
            }
        }
    }
}
