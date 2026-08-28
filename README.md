# wt

`wt` is a tool for git worktrees. It also controls the files that git ignores.

## The problem

The command `git worktree add` makes a new checkout. It does not make the
files that git ignores. Each new worktree is therefore not
complete.

`wt` keeps one file and makes a link in each worktree. A git hook makes the
links.

## How to use `wt`

### Start with a repository that you do not have

```sh
wt clone git@github.com:you/project.git
cd project/main
```

The command makes this structure:

```
project/
├── .bare/    the bare repository. It holds the store and the hook
├── .git      the text `gitdir: ./.bare`
└── main/     the first worktree
```

### Start with a repository that is already on your disk

Go into a worktree of the repository. Make the store, the config entry and
the hook:

```sh
wt init
```

Then move each ignored file into the store:

```sh
wt share .env .auth .envrc
```

The command moves each file one time. It then makes a link in each worktree
that is already present. One command is therefore sufficient for the full
repository.

The command refuses a path that contains a tracked file. Git supplies each
tracked file, and a link hides it. Give the ignored path instead. For example,
give `terraform/terraform.tfvars` and not `terraform/`.

### Make a worktree

```sh
wt add eng-1234
```

The hook makes the links in the new worktree. The program that makes the
worktree is not important. `wt add`, `git worktree add`, your editor and an
agent all start the hook.

The command finds the branch in this sequence:

1. A local branch.
2. A remote branch `origin/eng-1234`.
3. A fetch of `origin/eng-1234` from the network.
4. A new branch from the default branch.

Steps 1 and 2 do not use the network. The command does step 3 only if steps 1
and 2 find no branch. Step 3 prevents an error: without it, `wt` can make a
local branch while a remote branch with the same name is already present.

Use the option `--no-fetch` to stay offline. Use the option `--fetch` to do a
fetch first.

### Go to a worktree

```sh
wt deny
```

`wt <worktree>` is the short form of `wt cd <worktree>`. Both accept the name
of the worktree directory. Both also accept the name of the branch, because a
branch can move after you make the worktree. The start of a directory name is
sufficient when only one worktree matches.

A command wins over a worktree with the same name. `wt ls` therefore prints
the worktrees, also when a worktree has the name `ls`. The long form
`wt cd ls` goes to that worktree, and `wt add` prints a warning when it makes
a directory with the name of a command.

### See the worktrees

```sh
wt ls
```

The command prints each worktree, the branch it is on, and how many links are
correct. It changes nothing.

### Repair the links

The command `git clean -xdf` removes a link. A worktree can also be older than
the config. The command `wt sync` makes each link that is absent:

```sh
wt sync
```

### Remove a worktree

```sh
wt delete eng-1234
```

The command removes the worktree. It then deletes the branch if git merged the
branch.

### `wt` does not destroy a file that is different

The commands `wt share` and `wt sync` compare the bytes before they make a
link.

| Condition | Result |
|---|---|
| The two files are the same. | `wt` replaces the file with a link. |
| The two files are different. | `wt` keeps the file and prints a message. |

Use the option `--force` to replace a file that is different.

### Commands

| Command | Function |
|---|---|
| `wt init` | Make the store, the config entry and the hook. Move no files. |
| `wt share <path>…` | Move ignored paths into the store. Link them in each worktree. |
| `wt add <branch> [dir]` | Make a worktree for a branch. |
| `wt cd <worktree>` | Print the path of a worktree. The shell integration then changes directory. |
| `wt <worktree>` | The short form of `wt cd <worktree>`. |
| `wt ls` | Print each worktree, its branch and the condition of its links. |
| `wt sync` | Compare each worktree with the config. Make the links that are absent. |
| `wt delete <worktree>` | Remove a worktree. Delete its branch if git merged the branch. |
| `wt clone <url> [dir]` | Clone into the `.bare` layout. Make the first worktree. |
| `wt shell-init fish` | Print the shell integration. |

### Config file

One global file holds the shared paths for each repository:

```toml
# ~/.config/wt/config.toml
[repos."github.com/you/project"]
link = [".env.override", ".auth", ".envrc"]
```

The key is the remote URL in a normal form. Each form of the URL gives the
same key, so one file is correct for SSH and for HTTPS, and on each machine.
Put this file in your dotfiles to keep the list.

`wt` reads `$XDG_CONFIG_HOME/wt/config.toml` when that variable is set.

The file holds no secret. It holds only path names. The content stays in the
store, and the store stays in the repository at `<common-directory>/shared/`.

## Installation

### Cargo

```sh
cargo build --release
ln -sf "$PWD/target/release/wt" ~/.local/bin/wt
```

### Nix

```sh
nix profile install github:lorenzolfm/wt
```

## Shell integration

Add this line to `config.fish`:

```fish
wt shell-init fish | source
```

The integration also gives completion, and it changes the directory after
`wt add`, `wt clone`, `wt cd` and `wt <worktree>`:

| Position | Candidates |
|---|---|
| `wt <TAB>` | each command and each worktree |
| `wt add <TAB>` | each local branch and each remote branch |
| `wt delete <TAB>` | each worktree |
| `wt cd <TAB>` | each worktree |
| `wt share <TAB>` | each ignored path that the config does not have, and files |

## License

MIT
