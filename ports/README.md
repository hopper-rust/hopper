# Finished ports

Fabric mods someone has ported to Horizon, **tested in game** and published on the
[Horizon registry](https://github.com/horizon-rust/horizon-mods). When someone checks one of
these Fabric mods, Hopper shows "Already ported" and points to the finished mod instead of
starting a new port.

One file per port, named after its registry id: `ports/<registry-id>.toml`.

```toml
[fabric]
modrinth = "showfps"          # the Fabric mod it replaces (its Modrinth slug)
versions = ["1.4.0"]          # optional: the Fabric versions it covers

[horizon]
registry = "showfps"          # its id on the Horizon registry
builds = "Nov 22+"            # the Horizon builds it works on, in words

[info]
ported_by = "your-name"
tested = "2026-11-25"         # when you last tested it in game
notes = "Same features; the settings screen is Horizon-style."   # optional
```

Rules:

- The port is on the registry already: the PR adding it here links to it.
- It was tested in game; update `tested` and `builds` when you test it on a newer build.
- It credits the original mod and keeps its license, like every Hopper port.
- A mod "inspired by" a closed-license mod can be listed too: say so in `notes`.
