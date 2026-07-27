# git sidecar sync

```
git sidecar sync [<sidecar-name>] [--standalone | --unify]
```

`git sidecar sync` brings every configured sidecar's on-disk state in line with the config. Pass a sidecar name to sync just that one. For each sidecar it:

- clones it if it is not present (a missing or empty directory counts as not present), into the [layout](../layout.md) the config calls for;
- restores the working tree from the external git directory if a unified sidecar's mapping directory was deleted but its git dir survived;
- **relocates the git dir of an already-cloned sidecar** that still has `.git` inside its mapping directory (an old-layout checkout) out to the unified location — unless that sidecar is marked `standalone` in config, in which case it is left completely alone.

> **Behavior change from earlier versions:** sidecars that are already cloned are no longer always "left untouched". A routine `sync` will move an old-layout sidecar's git directory to `<parent>/.git/git-sidecar/<name>/gitdir/` — the working tree, history, and content are unaffected; only where the git dir physically lives changes. Mark a sidecar `standalone = true` (or clone it with `--standalone`) if you want it left exactly as-is.

Before any relocation, the sidecar's `remote.origin.url` is checked against the configured `repo` — a mismatch prints a warning and skips the sidecar. If a mapping directory exists and is non-empty but isn't a git repository anywhere, it is skipped with a warning. Any warning or failed step makes the command exit non-zero. Running `sync` again on an already-synced sidecar is a no-op.

`sync` also makes sure every present sidecar is listed in the parent repo's [exclude file](../exclude-file.md), so sidecar directories never show up in the parent's `git status`.

## Changing a sidecar's layout

With `--standalone` (requires a sidecar name), the sidecar is marked `standalone = true` in config, and if it is currently unified its git directory is moved back inside the mapping directory. From then on, flag-less `sync` runs leave its layout alone — the config setting, not the flag, is what protects it.

With `--unify` (requires a sidecar name), the reverse: the `standalone` marking is cleared and the git directory moves back out to the unified location. `--standalone` and `--unify` cannot be combined.
