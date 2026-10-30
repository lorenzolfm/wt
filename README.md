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

The command also accepts the link to a pull request on GitHub:

```sh
wt add https://github.com/you/project/pull/123
```

The result is the same as `wt add` with the name of the branch of the pull
request. Git has no ref for that name. Origin has the ref
`refs/pull/123/head`, which holds the commit. `wt` asks origin for that ref
and for each branch, and it finds the branch at that commit.

`wt` refuses a pull request when more than one branch is at its commit. Give
the branch name in that case. It also refuses a pull request whose branch
origin deleted. A link always uses the network, so `--no-fetch` refuses it.

The branch of a pull request is on origin with the same name, so its upstream
branch must be `origin/<branch>`. A local branch can track a different branch.
For example, `git worktree add -b eng-1234 <path> origin/master` makes a
branch that tracks master, and `git push` then refuses to push it. A link
makes `origin/<branch>` the upstream branch of the local branch, also when a
worktree has the branch already.

A pull request can come from a fork. Origin then has no branch at its commit,
and git cannot name the fork. `wt` asks GitHub with
[`gh`](https://cli.github.com), so a pull request from a fork needs `gh` and
`gh auth login`. A pull request from origin does not need `gh`.

`wt` adds a remote for the fork with the name of its owner, for example
`alice`. The URL has the form of the URL of origin, so SSH stays SSH. If a
remote has the URL of the fork already, `wt` uses that remote. When more than
one remote has it, `wt` uses the remote that the branch of the pull request
tracks, so the same pull request always gives the same branch. The local
branch has the name of the branch in the fork, and it tracks
`alice/<branch>`. When a local branch has that name and tracks a different
branch, for example `main`, the local branch is `alice-main`.

The link can also point to the repository that origin is a fork of. For
example, origin is `you/project`, and the pull request is in
`upstream/project`. Origin has no ref for that pull request, so `wt` asks
GitHub for its branch, and the link needs `gh`. The branch can be in origin, in
`upstream/project` or in a different fork. `wt` uses the remote that has the
URL of that repository, or it adds one as for a fork. `wt` refuses a link to a
repository that origin is not a fork of.

`wt prune` fetches from origin only, so it does not learn that a pull request
from a fork is finished. Remove its worktree with `wt delete`.

A branch can be in one worktree only. When a worktree has the branch already,
`wt add` takes you to that worktree instead of making a new one.

The second argument names the directory of the worktree, for example
`wt add eng-1234 review`. The default is the branch name with `-` for `/`.
The directory is beside the other worktrees, so `wt` refuses a path such as
`../review` or `nested/review`.

### Go to a worktree

```sh
wt deny
```

`wt <worktree>` is the short form of `wt cd <worktree>`. Both accept the name
of the worktree directory. Both also accept the name of the branch, because a
branch can move after you make the worktree. The start of a directory name is
sufficient when only one worktree matches.

A command wins over a worktree with the same name, and a short form such as
`wt d` is a command. `wt ls` therefore prints the worktrees, also when a
worktree has the name `ls`. The long form `wt cd ls` goes to that worktree,
and `wt add` prints a warning when the worktree that it makes or finds has the
name of a command.

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
wt delete eng-1234 eng-1235 eng-1236
```

The command removes each worktree. It then deletes the branch if git merged the
branch. `wt d eng-1234` is the short form.

The command finds each name before it removes anything, so a name that is wrong
changes nothing. If git refuses a worktree, the command keeps it, removes the
others and then fails.

You can be in a worktree that the command removes. The shell integration then
takes you to the worktree of the default branch, for example `main`. If no
worktree has the default branch, it takes you to the directory that holds the
worktrees.

### Remove each worktree whose work is finished

```sh
wt prune
```

The command asks origin which branches are still present. It then removes
each worktree whose work is complete, and it deletes the branch:

| Condition | Example |
|---|---|
| The base branch contains the branch, and origin still has it. | A merge commit, and the branch is not deleted yet. |
| Origin no longer has the upstream branch. | A squash merge or a rebase merge, and the branch is deleted. |

The second condition is the one that matters for a squash merge. Such a merge
writes a new commit, so the branch is not an ancestor of the base branch, and
the merge is complete nonetheless.

`wt prune` removes no branch that is only on your machine. Each condition
above needs proof that the branch reached origin: origin has it now, or the
config names an upstream branch that origin no longer has. A new branch from
`wt add` holds no commit of its own, so the base branch contains it, and the
test for a merge alone cannot tell that branch from finished work. The proof
of a push is the rule that keeps `wt add eng-1234` safe until you push it.

The command also keeps:

| Condition | Reason |
|---|---|
| The worktree has a modified file or an untracked file. | Use `--force`. |
| You are in the worktree. | A removal would leave your shell in a path that is absent. |
| The worktree is on the base branch, or on no branch. | There is no branch to test. |

Use `--dry-run` to read the list and change nothing. Use `--no-fetch` to stay
offline; the command then uses the refs it already has.

The command also removes the administrative files of a worktree whose
directory you deleted by hand.

Git does not count the files that it ignores. An ignored file that is not in
the store is therefore lost without a warning, as in `wt delete`.

### `wt` does not destroy a file that is different

The commands `wt share` and `wt sync` compare the bytes before they make a
link.

| Condition | Result |
|---|---|
| The two files are the same. | `wt` replaces the file with a link. |
| The two files are different. | `wt` keeps the file and prints a message. |

Use the option `--force` to replace a file that is different.

### Commands

| Command | Short | Function |
|---|---|---|
| `wt init` | | Make the store, the config entry and the hook. Move no files. |
| `wt share <path>…` | `wt sh` | Move ignored paths into the store. Link them in each worktree. |
| `wt add <branch> [dir]` | `wt a` | Make a worktree for a branch or for the link to a pull request. |
| `wt cd <worktree>` | `wt c` | Print the path of a worktree. The shell integration then changes directory. |
| `wt <worktree>` | | The short form of `wt cd <worktree>`. |
| `wt ls` | `wt l` | Print each worktree, its branch and the condition of its links. |
| `wt sync` | `wt s` | Compare each worktree with the config. Make the links that are absent. |
| `wt delete <worktree>...` | `wt d` | Remove each worktree. Delete its branch if git merged the branch. |
| `wt prune` | `wt p` | Remove each worktree whose work is finished. Delete its branch. |
| `wt clone <url> [dir]` | | Clone into the `.bare` layout. Make the first worktree. |
| `wt shell-init fish` | | Print the shell integration. |

A short form is the command itself, so it also wins over a worktree with the
same name: `wt d` runs `wt delete`, and `wt cd d` goes to a worktree named
`d`. `wt init` and `wt clone` have no short form. Each is short already, and
you run it one time for a repository.

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

The case of the key does not matter, because GitHub ignores it. A clone of
`you/Project` uses the entry for `you/project`. `wt` refuses a file with two
keys that differ only in case. They are the same repository, so merge their
lists into one entry.

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

Keep the name `wt`. The hook points to the program, and `wt init` refuses a
program with a different name. `wt` also shows a warning for a hook that
points to a program with a different name.

## Shell integration

Add this line to `config.fish`:

```fish
wt shell-init fish | source
```

The integration also gives completion, and it changes the directory after
`wt add`, `wt clone`, `wt cd` and `wt <worktree>`. It also changes the
directory after `wt delete` removes the worktree that you are in:

| Position | Candidates |
|---|---|
| `wt <TAB>` | each command, each short form and each worktree |
| `wt add <TAB>`, `wt a <TAB>` | each local branch and each remote branch |
| `wt delete <TAB>`, `wt d <TAB>` | each worktree |
| `wt cd <TAB>`, `wt c <TAB>` | each worktree |
| `wt share <TAB>`, `wt sh <TAB>` | each ignored path that the config does not have, and files |

## License

MIT
