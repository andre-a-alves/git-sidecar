# Commands Overview

```
git sidecar <sidecar-name> <git-command> [args...]
git sidecar <git-command> [args...]              # from inside a sidecar's directory
```

`git-sidecar` finds the parent git repository, loads its OS [config file](../configuration.md), then runs the given git command against the sidecar's repository (using its `mapping` working tree and wherever its git directory lives). You can run it from anywhere inside your project.

When the current directory is inside a sidecar's mapping directory, the name may be omitted — the command applies to the sidecar you are standing in. An explicit name always wins over the current directory, so you can address any sidecar from anywhere. Outside of any sidecar, omitting the name is an error.

```sh
# List branches of the sidecar repo
git sidecar foobar branch

# Pull latest changes
git sidecar foobar pull

# From inside .vendor/foobar/: same thing, no name needed
git sidecar log --oneline
git sidecar status
```

Any git command and its arguments are passed through as-is.

## Subcommands

In addition to the pass-through form above, `git sidecar` has four subcommands for managing sidecars themselves:

| Subcommand | Purpose |
| ---------- | ------- |
| [`clone`](./clone.md) | Clone a repo into your project and register it as a sidecar |
| [`list`](./list.md) | Show configured sidecars |
| [`sync`](./sync.md) | Bring on-disk state in line with the config |
| [`remove`](./remove.md) | Unregister a sidecar (alias: `rm`) |

Because `list`, `sync`, `clone`, `remove`, `rm`, and `help` are subcommands, they are reserved and cannot be used as sidecar nicknames.
