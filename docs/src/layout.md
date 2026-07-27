# Sidecar Layout

A sidecar can live on disk in one of two layouts: **unified** (the default) or **standalone**.

## Unified (default)

By default a sidecar is **unified**: its actual git directory does not live at `<mapping>/.git` but inside the parent's git directory, at

```
<parent>/.git/git-sidecar/<nickname>/gitdir/
```

and the mapping directory holds only the sidecar's working tree — no `.git` at all. This path is derived from the nickname; it is not configurable.

### Why?

The reason is a discovery trap. Git finds "the repository" by walking upward from the current directory to the nearest `.git`. With a traditional checkout, that walk stops at the sidecar the moment you `cd` inside it — plain `git status` or `git log` silently operates on the sidecar, and there is no way to touch the parent repo without `cd`-ing back out.

With the unified layout there is no `.git` inside the mapping directory, so git's walk continues up to the parent: **bare `git` inside a sidecar transparently operates on the parent repo**, treating the sidecar's directory like any other subdirectory. (Its contents stay hidden from the parent's `git status` via the [exclude file](./exclude-file.md).) To address the sidecar itself, use `git sidecar` — which from inside a sidecar's directory doesn't even need the name (see [Commands](./commands/index.md)).

## Standalone

A sidecar marked `standalone = true` in the [config](./configuration.md) opts out: it keeps the traditional `<mapping>/.git`, is never touched by the relocation logic, and inside its directory plain git addresses the sidecar, as before.

You can switch a sidecar between the two layouts at any time with [`git sidecar sync --standalone` / `--unify`](./commands/sync.md).
