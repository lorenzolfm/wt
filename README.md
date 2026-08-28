# wt

`wt` is a tool for git worktrees. It also controls the files that git ignores.

## The problem

The command `git worktree add` makes a new checkout. It does not make the
files that git ignores. Examples of these files are `.env.override`,
credentials and `terraform.tfvars`. Each new worktree is therefore not
complete.

The usual solution is a shell script. The script copies the files from one
worktree into the new worktree. The result is many copies of each file. If you
change one copy, the other copies are then not correct.

`wt` keeps one file and makes a link in each worktree. A git hook makes the
links.

## Definitions

| Term | Definition |
|---|---|
| worktree | A checkout that git makes with `git worktree add`. |
| the store | The directory `shared/`. It holds one copy of each shared file. |
| the manifest | The file `worktree-shared.toml`. It lists the shared paths. |
| the hook | The git hook `post-checkout`. It makes the links. |
| a link | A symbolic link from a worktree to a file in the store. |
| the common directory | The directory that git shares between all worktrees. |

## How the tool operates

### The hook makes the links

The command `wt init` puts a link at `hooks/post-checkout`. The link points to
the `wt` program.

Git runs the hook after git makes a worktree. Git sets the current directory to
the new worktree. The program reads its own name. If the name is
`post-checkout`, the program starts the seed operation.

The program then reads the first argument. Git gives the previous HEAD as the
first argument. A null SHA shows that git made a new worktree. For all other
values the program stops immediately.

This method controls every worktree. The program that makes the worktree is not
important. `wt add` makes worktrees. `git worktree add` makes worktrees. Your
editor can make worktrees. An agent can make worktrees. The hook operates for
all of them.

### The tool makes links, not copies

Each shared path has one file in the store. Each worktree has a link to that
file. To change a credential, write the file one time.

Links are also safer than copies. The command `git clean -xdf` removes the
link, but it keeps the file in the store. The command `rm -rf` on a worktree
also removes only the link.

### The store and the manifest

The store has the same structure as a worktree. A file at `.auth/cred.json` in
a worktree is at `shared/.auth/cred.json` in the store.

The manifest gives the list of shared paths:

```toml
# <common-directory>/worktree-shared.toml
link = [".env.override", ".auth", ".envrc"]
```

The tool does not search the store to find the paths. A search cannot show the
correct depth for each link. These two examples show the problem:

- `.auth/` — git ignores all of this directory. Link the directory. Each new
  file in the directory is then available in each worktree.
- `terraform/terraform.tfvars` — git ignores only this file. The directory
  `terraform/` contains tracked files. Link only the file. If you link the
  directory, you hide the tracked files.

The command `wt share` refuses a path that contains tracked files.

## Commands

| Command | Function |
|---|---|
| `wt init` | Make the store, the manifest and the hook. Move no files. |
| `wt share <path>…` | Move ignored paths into the store. Link them in each worktree. |
| `wt add <branch> [dir]` | Make a worktree for a branch. |
| `wt sync` | Compare each worktree with the manifest. Make the links that are absent. |
| `wt delete <worktree>` | Remove a worktree. Delete its branch if git merged the branch. |
| `wt clone <url> [dir]` | Clone into the `.bare` layout. Make the first worktree. |
| `wt shell-init fish` | Print the shell integration. |

Use the option `--repo <path>` to select a different repository.

The tool accepts two layouts. In the first layout the bare repository is the
parent directory (`repo.git/<worktree>`). In the second layout the bare
repository is `.bare`, and the worktrees are beside it.

## The tool does not destroy a different file

The commands `wt share` and `wt sync` compare the bytes of the two files
before they make a link.

| Condition | Result |
|---|---|
| The two files are the same. | The tool replaces the file with a link. |
| The two files are different. | The tool keeps the file and prints a message. |

To replace a different file, use the option `--force`.

## The command `wt add` uses the network only when necessary

The command finds the branch in this sequence:

1. A local branch.
2. A remote branch `origin/<branch>`.
3. A fetch of `origin/<branch>` from the network.
4. A new branch from the default branch.

Steps 1 and 2 do not use the network. The command does step 3 only if steps 1
and 2 find no branch.

Step 3 prevents an error. A remote branch can be present, but your last
fetch was before it. Without step 3, the command makes a new local branch
with the same name. The two branches then diverge. You find the problem when
you push the branch.

Use the option `--no-fetch` to remove step 3. Use the option `--fetch` to do a
fetch before step 1.

The tool gives the new branch no upstream branch. This prevents a push to the
default branch.

## Installation

### With cargo

```sh
cargo build --release
ln -sf "$PWD/target/release/wt" ~/.local/bin/wt
```

### With nix

```sh
nix profile install github:lorenzolfm/wt
```

For a development shell, use `nix develop`. The repository contains an `.envrc`
file for direnv.

### Shell integration

A program cannot change the directory of the shell that started it. The shell
integration does this operation. Add this line to `config.fish`:

```fish
wt shell-init fish | source
```

The integration also gives completion for branch names and worktree names.

### The hook needs a path that does not change

A git hook stays in the repository for longer than the program. Do not use a
`/nix/store` path for the hook. The hash in that path changes with each build,
and nix then removes the old path. Git ignores a hook that it cannot run and
shows no error. The seed operation stops, and you see no message.

The command `wt init` refuses to write a `/nix/store` path. It uses the first
of these paths that is present:

1. `~/.local/bin/wt`
2. `~/.nix-profile/bin/wt`
3. `/run/current-system/sw/bin/wt`

The commands `wt add` and `wt sync` examine the hook. They print a warning if
the hook points to a path that is not present.

## How to use the tool with a repository that is already present

The tool has no command to move an existing repository. Use `wt init` and then
`wt share`:

```sh
cd repo.git/master
wt init
wt share .env.override .auth .envrc
```

The command `wt share` also makes the links in each worktree that is already
present. One operation is therefore sufficient for the full repository.

## Limits of version 1

Version 1 does not have these functions:

- Copy mode. All shared paths use links. The manifest format can accept a copy
  mode later.
- An examination of the ignored files during `wt delete`.
- A list of the known repositories.
- A command to move a different copy from a worktree into the store.

## License

MIT
