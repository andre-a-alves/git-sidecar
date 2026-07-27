# Configuration

Configuration is stored outside the project, under your OS config directory:

- **Linux:** `$XDG_CONFIG_HOME/git-sidecar`, or `~/.config/git-sidecar` when `XDG_CONFIG_HOME` is not set
- **macOS:** `~/Library/Application Support/git-sidecar`
- **Windows:** `%APPDATA%\git-sidecar`

`git-sidecar` identifies the current project from the nearest git repository's `remote.origin.url`. The remote URL is normalized into a repo-shaped config path. For example, a parent repo with this origin:

```
git@github.com:andre-a-alves/git-sidecar.git
```

uses this config file on Linux when `XDG_CONFIG_HOME` is not set:

```
~/.config/git-sidecar/github.com/andre-a-alves/git-sidecar/config.toml
```

## File format

The config file must use version `1`:

```toml
version = 1

[sidecars.foobar]
repo = "git@github.com:example/foobar.git"
mapping = ".vendor/foobar/"
standalone = true   # optional; omitted/false = unified (the default)
```

- **`version`** — the config file version; currently only `1` is supported
- **`sidecars.foobar`** — the nickname you use on the command line
- **`repo`** — the remote URL for the sidecar repository
- **`mapping`** — path to the directory containing the sidecar git repository, relative to the parent repo root
- **`standalone`** — optional; when `true`, the sidecar keeps its own `.git` inside the mapping directory instead of the [unified layout](./layout.md). Absent means `false`, so config files written before this field existed keep working unchanged.

You can define as many `[sidecars.<nickname>]` entries as you need.

> **Tip:** you don't have to write this file by hand — [`git sidecar clone`](./commands/clone.md) creates and extends it for you.

Because `list`, `sync`, `clone`, `remove`, `rm`, and `help` are subcommands, they are reserved and cannot be used as sidecar nicknames.
