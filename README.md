# git-sidecar

[![Crates.io](https://img.shields.io/crates/v/git-sidecar.svg)](https://crates.io/crates/git-sidecar)
[![CI](https://github.com/andre-a-alves/git-sidecar/actions/workflows/ci.yml/badge.svg)](https://github.com/andre-a-alves/git-sidecar/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/andre-a-alves/git-sidecar#license)

Run git commands against sidecar repositories that live inside your working directory.

A sidecar repo is a separate git repository checked out inside another project — useful for keeping vendored code, generated outputs, or loosely related projects alongside your main repo without making them submodules.

## Installation

```
cargo install git-sidecar
```

This installs the `git-sidecar` binary. Git automatically treats any `git-<name>` binary on your `PATH` as a subcommand, so it becomes available as `git sidecar`.

## Configuration

Configuration is stored outside the project, under your OS config directory:

- Linux: `$XDG_CONFIG_HOME/git-sidecar`, or `~/.config/git-sidecar` when `XDG_CONFIG_HOME` is not set
- macOS: `~/Library/Application Support/git-sidecar`
- Windows: `%APPDATA%\git-sidecar`

`git-sidecar` identifies the current project from the nearest Git repository's `remote.origin.url`.
The remote URL is normalized into a repo-shaped config path. For example, a parent repo with this origin:

```
git@github.com:andre-a-alves/git-sidecar.git
```

uses this config file on Linux when `XDG_CONFIG_HOME` is not set:

```
~/.config/git-sidecar/github.com/andre-a-alves/git-sidecar/config.toml
```

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
- **`standalone`** — optional; when `true`, the sidecar keeps its own `.git` inside the mapping directory instead of the unified layout described below. Absent means `false`, so config files written before this field existed keep working unchanged.

You can define as many `[sidecars.<nickname>]` entries as you need. You don't have to write this file by hand — `git sidecar clone` creates and extends it for you.

## How sidecars are laid out on disk

By default a sidecar is **unified**: its actual git directory does not live at `<mapping>/.git` but inside the parent's git directory, at

```
<parent>/.git/git-sidecar/<nickname>/gitdir/
```

and the mapping directory holds only the sidecar's working tree — no `.git` at all. This path is derived from the nickname; it is not configurable.

The reason is a discovery trap. Git finds "the repository" by walking upward from the current directory to the nearest `.git`. With a traditional checkout, that walk stops at the sidecar the moment you `cd` inside it — plain `git status` or `git log` silently operates on the sidecar, and there is no way to touch the parent repo without `cd`-ing back out. With the unified layout there is no `.git` inside the mapping directory, so git's walk continues up to the parent: **bare `git` inside a sidecar transparently operates on the parent repo**, treating the sidecar's directory like any other subdirectory (its contents stay hidden from the parent's `git status` via the exclude file). To address the sidecar itself, use `git sidecar` — which from inside a sidecar's directory doesn't even need the name (see below).

A sidecar marked `standalone = true` opts out: it keeps the traditional `<mapping>/.git`, is never touched by the relocation logic, and inside its directory plain git addresses the sidecar, as before.

## Usage

```
git sidecar <sidecar-name> <git-command> [args...]
git sidecar <git-command> [args...]              # from inside a sidecar's directory
```

`git-sidecar` finds the parent Git repository, loads its OS config file, then runs the given git command against the sidecar's repository (using its `mapping` working tree and wherever its git directory lives). You can run it from anywhere inside your project.

When the current directory is inside a sidecar's mapping directory, the name may be omitted — the command applies to the sidecar you are standing in. An explicit name always wins over the current directory, so you can address any sidecar from anywhere. Outside of any sidecar, omitting the name is an error.

```
# List branches of the sidecar repo
git sidecar foobar branch

# Pull latest changes
git sidecar foobar pull

# From inside .vendor/foobar/: same thing, no name needed
git sidecar log --oneline
git sidecar status
```

Any git command and its arguments are passed through as-is.

### Listing sidecars

```
git sidecar list [--global]
```

`git sidecar list` shows the sidecars configured for the current repo — one per line with the nickname, remote URL, mapping, and layout (`unified` or `standalone`). With `--global`, it instead walks the whole config directory and lists every configured sidecar, grouped under a `<host>/<owner>/<repo>:` header identifying the parent repo.

### Syncing sidecars

```
git sidecar sync [<sidecar-name>] [--standalone | --unify]
```

`git sidecar sync` brings every configured sidecar's on-disk state in line with the config. Pass a sidecar name to sync just that one. For each sidecar it:

- clones it if it is not present (a missing or empty directory counts as not present), into the layout the config calls for;
- restores the working tree from the external git directory if a unified sidecar's mapping directory was deleted but its git dir survived;
- **relocates the git dir of an already-cloned sidecar** that still has `.git` inside its mapping directory (an old-layout checkout) out to the unified location — unless that sidecar is marked `standalone` in config, in which case it is left completely alone.

> **Behavior change from earlier versions:** sidecars that are already cloned are no longer always "left untouched". A routine `sync` will move an old-layout sidecar's git directory to `<parent>/.git/git-sidecar/<name>/gitdir/` — the working tree, history, and content are unaffected; only where the git dir physically lives changes. Mark a sidecar `standalone = true` (or clone it with `--standalone`) if you want it left exactly as-is.

Before any relocation, the sidecar's `remote.origin.url` is checked against the configured `repo` — a mismatch prints a warning and skips the sidecar. If a mapping directory exists and is non-empty but isn't a git repository anywhere, it is skipped with a warning. Any warning or failed step makes the command exit non-zero. Running `sync` again on an already-synced sidecar is a no-op.

With `--standalone` (requires a sidecar name), the sidecar is marked `standalone = true` in config, and if it is currently unified its git directory is moved back inside the mapping directory. From then on, flag-less `sync` runs leave its layout alone — the config setting, not the flag, is what protects it.

With `--unify` (requires a sidecar name), the reverse: the `standalone` marking is cleared and the git directory moves back out to the unified location. `--standalone` and `--unify` cannot be combined.

`sync` also makes sure every present sidecar is listed in the parent repo's exclude file (see below), so sidecar directories never show up in the parent's `git status`.

### Cloning a new sidecar

```
git sidecar clone <repo-url> [<directory>] [--name <nickname>] [--standalone]
```

`git sidecar clone` clones a repo into the parent repository and registers it as a sidecar — creating the config file if it doesn't exist yet. The nickname and directory both default to the repository name from the URL:

```
# Clones into ./foobar, registers as [sidecars.foobar]
git sidecar clone git@github.com:example/foobar.git

# Clones into .vendor/fb, registers as [sidecars.fb]
git sidecar clone git@github.com:example/foobar.git .vendor/fb --name fb
```

The directory is resolved relative to where you run the command (like `git clone`), and the stored `mapping` is computed relative to the parent repo root. If the nickname or mapping is already taken, or the target directory is non-empty, the command refuses and changes nothing. Existing config file content is preserved — the new entry is appended.

New sidecars are cloned in the unified layout by default. Pass `--standalone` to clone the traditional way (`.git` inside the mapping directory) and record `standalone = true` in the config.

`clone` also adds the new directory to the parent repo's exclude file.

### Removing a sidecar

```
git sidecar remove <sidecar-name> [--delete]
git sidecar rm <sidecar-name> [--delete]      # alias
```

`git sidecar remove` deletes the sidecar's entry from the config file and its line from the exclude file's managed block. The cloned directory is left on disk by default — for a unified sidecar, its git directory is moved back inside the mapping directory first, so what remains is an ordinary standalone git repository. Pass `--delete` to remove the directory (and, for unified sidecars, the external git directory) as well — careful: this discards any unpushed work in the sidecar.

Because `list`, `sync`, `clone`, `remove`, `rm`, and `help` are subcommands, they are reserved and cannot be used as sidecar nicknames.

### The exclude file

`clone` and `sync` keep sidecar directories out of the parent repo's `git status` by adding them to `.git/info/exclude` (the local, uncommitted counterpart to `.gitignore`). Entries live in a managed block, and anything outside it is never touched:

```
# >>> git-sidecar (managed) >>>
/foobar/
/.vendor/dep2/
# <<< git-sidecar (managed) <<<
```

`clone` and `sync` only add entries; `git sidecar remove` deletes a sidecar's entry. If you edit the config by hand instead, clean up the block yourself.

## License

This project is licensed under either the [MIT License](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE), at your option.
