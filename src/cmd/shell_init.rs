use anyhow::{Result, bail};

/// Print the shell integration.
///
/// A program cannot change the directory of the shell that started it. The
/// commands `add`, `clone`, `cd` and `delete` therefore print the new path.
/// The function below reads that path and changes the directory. The program
/// makes this shell code. Do not write it manually.
pub fn run(shell: &str) -> Result<()> {
    match shell {
        "fish" => {
            print!("{FISH}");
            Ok(())
        }
        other => bail!("wt cannot make code for the shell {other}. wt supports fish only"),
    }
}

const FISH: &str = r#"# wt shell integration -- add to config.fish with:
#   wt shell-init fish | source

function wt --wraps wt --description 'git worktree manager'
    # add, clone, cd and the short form `wt <worktree>` print a path and
    # nothing else, so this function reads the path and changes directory.
    # delete prints a path only when it removes the worktree that the shell
    # is in. Every other command writes for the user and runs untouched. A
    # new command that writes for the user belongs in the first case, with
    # its short form beside it.
    switch "$argv[1]"
        case '' init share sh ls l sync s prune p shell-init help '-*' '__*'
            command wt $argv
        case '*'
            set -l dest (command wt $argv)
            # The status of wt, and not of the test: delete can print no
            # path and succeed, or print a path and fail.
            set -l st $status
            test -n "$dest" -a -d "$dest"; and cd $dest
            return $st
    end
end

complete -c wt -f
# Each command that has a short form gives both, so `wt d<TAB>` and
# `wt de<TAB>` both reach delete.
complete -c wt -n __fish_use_subcommand -a init       -d 'make the store, the config entry and the hook'
complete -c wt -n __fish_use_subcommand -a 'share sh' -d 'move ignored paths into the store and make the links'
complete -c wt -n __fish_use_subcommand -a 'add a'    -d 'make a worktree for a branch'
complete -c wt -n __fish_use_subcommand -a 'delete d' -d 'remove each worktree and delete its branch'
complete -c wt -n __fish_use_subcommand -a 'cd c'     -d 'change to a worktree'
complete -c wt -n __fish_use_subcommand -a 'ls l'     -d 'print each worktree, its branch and its links'
complete -c wt -n __fish_use_subcommand -a 'sync s'   -d 'compare each worktree with the config and make the links'
complete -c wt -n __fish_use_subcommand -a 'prune p'  -d 'remove each worktree whose work is finished'
complete -c wt -n __fish_use_subcommand -a clone      -d 'clone a repository into the .bare layout'
complete -c wt -n __fish_use_subcommand -a shell-init -d 'print the shell integration'

# `wt <worktree>` is the short form of `wt cd <worktree>`, so a worktree name
# is a candidate in the position of a command.
complete -c wt -n __fish_use_subcommand -a '(command wt __worktrees 2>/dev/null)'

complete -c wt -n '__fish_seen_subcommand_from add a'      -a '(command wt __branches 2>/dev/null)'
complete -c wt -n '__fish_seen_subcommand_from delete d'   -a '(command wt __worktrees 2>/dev/null)'
complete -c wt -n '__fish_seen_subcommand_from cd c'       -a '(command wt __worktrees 2>/dev/null)'

# `complete -c wt -f` above stops file completion for the whole command, so
# each subcommand that takes a path must ask for it again with -F.
complete -c wt -n '__fish_seen_subcommand_from share sh' -F \
    -a '(command wt __shareable 2>/dev/null)'
complete -c wt -n '__fish_seen_subcommand_from clone' -F
complete -c wt -l repo -r -F -d 'act on this repository'
"#;
