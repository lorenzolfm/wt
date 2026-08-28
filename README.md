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
├── .bare/    the bare repository. It holds the store, the manifest and the hook
├── .git      the text `gitdir: ./.bare`
└── main/     the first worktree
```

### Start with a repository that is already on your disk

Go into a worktree of the repository. Make the store, the manifest and the
hook:

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

### Repair the links

The command `git clean -xdf` removes a link. A worktree can also be older than
the manifest. The command `wt sync` makes each link that is absent:

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
| `wt init` | Make the store, the manifest and the hook. Move no files. |
| `wt share <path>…` | Move ignored paths into the store. Link them in each worktree. |
| `wt add <branch> [dir]` | Make a worktree for a branch. |
| `wt sync` | Compare each worktree with the manifest. Make the links that are absent. |
| `wt delete <worktree>` | Remove a worktree. Delete its branch if git merged the branch. |
| `wt clone <url> [dir]` | Clone into the `.bare` layout. Make the first worktree. |
| `wt shell-init fish` | Print the shell integration. |

### Config file

The manifest gives the list of shared paths:

```toml
# <common-directory>/worktree-shared.toml
link = [".env.override", ".auth", ".envrc"]
```

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

Gives completion for branch names and worktree names and `cd` into the worktree.

## License

MIT
