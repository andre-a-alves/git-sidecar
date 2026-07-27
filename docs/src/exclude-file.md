# The Exclude File

[`clone`](./commands/clone.md) and [`sync`](./commands/sync.md) keep sidecar directories out of the parent repo's `git status` by adding them to `.git/info/exclude` (the local, uncommitted counterpart to `.gitignore`). Entries live in a managed block, and anything outside it is never touched:

```
# >>> git-sidecar (managed) >>>
/foobar/
/.vendor/dep2/
# <<< git-sidecar (managed) <<<
```

`clone` and `sync` only add entries; [`git sidecar remove`](./commands/remove.md) deletes a sidecar's entry. If you edit the config by hand instead, clean up the block yourself.
