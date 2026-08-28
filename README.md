# wt

Git worktrees, plus the gitignored files they need to share.

`git worktree add` gives you a checkout. It does not give you `.env.override`,
credentials, or anything else git ignores — so every new worktree starts
subtly broken, and the usual fix is a `setup-worktree.sh` that copies files
from whichever worktree you decided was canonical.

`wt` replaces the copies with one file and N symlinks, and seeds them from a
`post-checkout` hook so it happens no matter who creates the worktree.

## How it works

**Seeding is hook-driven, never command-driven.** `wt init` symlinks
`hooks/post-checkout` at the `wt` binary. Git runs it with cwd set to the new
worktree; `wt` sees it was invoked as `post-checkout`, checks that the previous
HEAD is the null sha (a worktree add or clone, not a routine `git switch`), and
links everything in the manifest.

That covers every creation path — `wt add`, plain `git worktree add`, your
editor, an agent's worktree — rather than just the tool's own front door.

**Links, not copies.** Each shared path is one real file in `shared/` plus a
symlink per worktree. Rotating a credential is a single write. It is also the
safe option: `git clean -xdf` and `rm -rf worktree/` both remove the *link* and
leave the target intact.

**The store mirrors the worktree; the manifest states granularity.**

```toml
# <common-dir>/worktree-shared.toml
link = [".env.override", ".auth", ".envrc"]
```

The explicit list exists because a directory walk cannot distinguish `.auth/`
(wholly ignored → link the directory) from `terraform/terraform.tfvars` (an
ignored leaf inside a tracked tree → link the file, or you shadow the
checkout). `wt share` refuses any path with tracked files under it.

## Commands

| | |
|---|---|
| `wt init` | Scaffold `shared/`, the manifest, and the hook. Idempotent, moves nothing. |
| `wt share <path>…` | Move gitignored paths into the store, link them back everywhere. |
| `wt add <branch> [dir]` | Create a worktree. Resolves local → `origin/<branch>` → fetch → new branch. |
| `wt sync` | Reconcile every worktree against the manifest. |
| `wt delete <worktree>` | `git worktree remove` plus `git branch -d`. |
| `wt clone <url> [dir]` | Clone into the `.bare` layout and create the first worktree. |
| `wt shell-init fish` | Shell integration — `cd` into new worktrees, plus completions. |

`--repo <path>` acts on another repository. Both layouts work: the bare repo at
the root (`repo.git/<worktree>`) or `.bare` with worktrees as siblings.

### Divergence is never destroyed

`share` and `sync` compare bytes before replacing a real file with a link.
Identical → replaced. Different → skipped and reported, never overwritten.
`--force` overrides.

### Lazy fetch

`wt add` hits the network only when it has to: a known local branch or an
existing `origin/<branch>` resolves offline; only an unknown branch triggers a
targeted `git fetch origin <branch>`. This closes the hole where a stale
remote-tracking ref causes a *new* local branch to be silently created
alongside an existing remote one of the same name.

`--no-fetch` stays offline; `--fetch` refreshes first.

## Install

```sh
cargo build --release
ln -sf "$PWD/target/release/wt" ~/.local/bin/wt   # stable path for hooks
echo 'wt shell-init fish | source' >> ~/.config/fish/config.fish
```

The hook symlink must point at a **stable** path. On NixOS `current_exe()` is a
`/nix/store/<hash>` path that changes on rebuild and is garbage-collected — and
git skips a dangling hook *silently*, so seeding would stop working with no
error. `wt init` refuses to write a store path into a hook and uses
`~/.local/bin/wt` instead; `wt add` and `wt sync` warn if the hook dangles.

## Adopting an existing repo

There is no migration command — adoption is just `init` plus `share`:

```sh
cd repo.git/master
wt init
wt share .env.override .auth .envrc
```

`share` backfills every existing worktree, so one pass converts the whole repo.

## Not in v1

Copy mode (link-only for now; the manifest format reserves room for it),
a delete-time classifier for unshared gitignored data, diagnostics beyond
`sync`, a repo registry, and promoting a divergent worktree copy into the store.
