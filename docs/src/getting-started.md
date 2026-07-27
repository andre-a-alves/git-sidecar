# git-sidecar

<img class="sidecar-logo logo-on-light" src="img/git-sidecar-dark.svg" alt="git-sidecar logo">
<img class="sidecar-logo logo-on-dark" src="img/git-sidecar-light.svg" alt="git-sidecar logo">

Run git commands against sidecar repositories — separate git repos checked out *inside* your project, without making them submodules.

## 1. Install

```sh
cargo install git-sidecar
```

Git treats any `git-<name>` binary on your `PATH` as a subcommand, so this gives you `git sidecar`.

## 2. Clone a sidecar into your project

From anywhere inside your repo:

```sh
git sidecar clone git@github.com:example/foobar.git
```

## 3. Use it

```sh
git sidecar foobar status
git sidecar foobar pull
```

Or `cd` into the sidecar directory and drop the name:

```sh
cd foobar
git sidecar log --oneline
```

That's it — anything after the sidecar name is passed to git as-is.

## Where to next?

- [Configuration](./configuration.md) — where and how sidecars are registered
- [Sidecar Layout](./layout.md) — unified vs. standalone, and why there is no `.git` inside your sidecar
- [Commands](./commands/index.md) — `clone`, `list`, `sync`, `remove`
