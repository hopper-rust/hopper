# Contributing to Hopper

Thanks for helping! Hopper gets better one mapping, one rule and one port at a time.
Questions and ideas: the [Discord](https://discord.gg/kKqTbptz2T) or an issue.

There are four ways to help, from easiest to hardest:

1. [Add a library entry](#1-library-entries): one small TOML file.
2. [List a finished port](#2-finished-ports): one small TOML file.
3. [Add a converter rule](#3-converter-rules): Rust, with a test.
4. [Other code](#4-other-code): open an issue first.

Before you start, read the [rules](#rules-for-every-contribution) at the bottom. PRs that break them
are closed, however good the rest is.

## Setting up

You need [Rust](https://rustup.rs) (stable) and Python 3.11+.

```sh
pip install jsonschema
python scripts/check_library.py      # checks every library entry and port
cargo test                           # the checker, generator and converter tests
cargo fmt                            # formats the code (CI checks it)
cargo run -q -p hopper-cli -- check https://modrinth.com/mod/commandkeys
cargo run -q -p hopper-server        # the website on http://127.0.0.1:8080
```

CI runs the same checks on every pull request. A PR is only merged when they pass.

## 1. Library entries

An entry says "this Fabric thing is this Horizon thing". The format and the folders are in
[library/README.md](library/README.md).

1. Find the Fabric thing (event, class, method, mixin target) and what it becomes on Horizon.
2. Add one TOML file in the right folder of `library/`, named after the Fabric side in kebab case.
3. Run `python scripts/check_library.py`.
4. Open a pull request and say **how you checked** the mapping (see below).

Good entries:

- **One mapping per file.** Two Fabric events that both map to `client_tick_post` are two files.
- **Real names only.** Check every name, on both sides:
  - Fabric API: its source for the Minecraft version (Fabric renames things between versions: keep
    the old name as an alias).
  - Minecraft: the game's jar, with `javap -cp 26.3.jar <class>` (26.x jars aren't obfuscated). Old
    Yarn or intermediary names go in `aliases`.
  - Horizon: the hook must be in [core/data/hooks.txt](core/data/hooks.txt) (the check fails
    otherwise); host functions in the SDK's `src/host/`. Set `since_api` to the API you checked.
- **Honest status.** `direct` only when it behaves the same. If the Horizon side differs, use
  `partial` and say how in `notes`. If there's no API, use `missing` and link the Horizon issue
  asking for it (or `"unrequested"`).
- **Notes for a modder:** short, plain words, what to write on the Horizon side.

## 2. Finished ports

When you've ported a Fabric mod, tested it in game and published it on the
[Horizon registry](https://github.com/horizon-rust/horizon-mods), add it to `ports/` so Hopper
points people to it. Format and rules: [ports/README.md](ports/README.md).

## 3. Converter rules

The converter (`core/src/convert/`) turns Java inside Fabric event listeners into Rust, one
statement at a time. A statement is only converted when **every** part of it has a rule; anything
else stays as a `// TODO (Java):` line. Keep it that way: a wrong conversion that compiles is
worse than a TODO.

- `mod.rs`: which Fabric events it knows (`HookShape`: the hook, its parameters, what `return`
  becomes), key mappings, HUD elements.
- `java.rs`: the statements and expressions (`Translator::call` for method calls, `expr` for the
  rest).
- `commands.rs`: Brigadier client commands.
- `tests.rs`: one test per rule. Add a test with a small, made-up Java snippet (not copied from a
  mod) and the exact Rust you expect.

A good rule PR also says: which real mods use the pattern, and that a port using it **compiles**
against the SDK (`cargo-horizon-mod horizon-mod check`, or `cargo check` with the SDK in
`.horizon/sdk`).

## 4. Other code

The website (`server/`), the CLI (`cli/`), the checker and generator (`core/`): open an issue first,
so we agree on the approach before you write it.

When a new Horizon build comes out:

1. `scripts/update_hooks.py` refreshes `core/data/hooks.txt` from the new build's SDK, and
   `core/data/sdk.toml` gets its pins.
2. `scripts/update_builds.py <launcher native folder>` refreshes `core/data/builds.toml`, the builds
   people can pick on the site, from the SDK sources the Horizon launcher keeps
   (`%APPDATA%/horizon-launcher/native` on Windows). Each build is written as the newest one minus
   the hooks and host functions it doesn't have yet.

## Rules for every contribution

- **No AI inside Hopper.** Nothing in Hopper may call an AI model: it is plain rules and files.
- **Using AI to write a PR is up to you, but you are responsible for it.** Check every name and
  claim yourself, as above, and say how you checked. Made-up class names, hooks or methods get the
  PR closed. Keep PRs small and focused: one topic per PR.
- **Licenses.** Knowledge may come from your own ports, the Horizon mod API and docs, the Fabric API,
  Minecraft's class and method names, and mods whose license allows reading and reusing them. Never
  from mods that are all rights reserved or whose license forbids it: don't copy their code into
  entries, notes, tests or examples, and don't decompile mods.
- **No Mojang mapping files** in the repo. Class and method names are fine.
- **Honest output.** Hopper never makes something unfinished look finished.
- Be kind: see the [code of conduct](CODE_OF_CONDUCT.md).

By contributing, you agree that your contribution is under the [MIT License](LICENSE).
