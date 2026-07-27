# git sidecar list

```
git sidecar list [--global]
```

`git sidecar list` shows the sidecars configured for the current repo — one per line with the nickname, remote URL, mapping, and layout (`unified` or `standalone`).

With `--global`, it instead walks the whole [config directory](../configuration.md) and lists every configured sidecar, grouped under a `<host>/<owner>/<repo>:` header identifying the parent repo.
