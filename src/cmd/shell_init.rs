use anyhow::{Result, bail};

/// Print the shell integration.
///
/// A program cannot change the directory of the shell that started it. The
/// commands `add` and `clone` therefore print the new path. The function
/// below reads that path and changes the directory. The program makes this
/// shell code. Do not write it manually.
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
    switch "$argv[1]"
        case add clone
            set -l dest (command wt $argv)
            test -n "$dest" -a -d "$dest"; and cd $dest
        case '*'
            command wt $argv
    end
end

complete -c wt -f
complete -c wt -n __fish_use_subcommand -a init       -d 'scaffold shared store, manifest and hook'
complete -c wt -n __fish_use_subcommand -a share      -d 'move a gitignored path into the shared store'
complete -c wt -n __fish_use_subcommand -a add        -d 'create a worktree for a branch'
complete -c wt -n __fish_use_subcommand -a delete     -d 'remove a worktree and its branch'
complete -c wt -n __fish_use_subcommand -a sync       -d 'reconcile worktrees against the manifest'
complete -c wt -n __fish_use_subcommand -a clone      -d 'clone into the .bare worktree layout'
complete -c wt -n __fish_use_subcommand -a shell-init -d 'print shell integration'

complete -c wt -n '__fish_seen_subcommand_from add'    -a '(command wt __branches 2>/dev/null)'
complete -c wt -n '__fish_seen_subcommand_from delete' -a '(command wt __worktrees 2>/dev/null)'
"#;
