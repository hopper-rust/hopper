# Contributing to Hopper

Thanks for helping! Hopper is only as good as its library, and every entry makes the checker smarter.

## Adding a library entry

1. Find the Fabric thing (event, class, method, mixin target) and what it becomes on Horizon.
2. Add one TOML file in the right folder of `library/`. The format is in [library/README.md](library/README.md).
3. Open a pull request. Say how you know the mapping is right: ideally a port you tested in game.

Good entries:

- **One mapping per file.** Two Fabric events that both map to `client_tick_post` are two files.
- **Checked against the Horizon mod API.** Look the hook up in the SDK (`horizon-mod-vocab/src/hooks/`) or the host function in `src/host/`, and set `since_api` to the version you checked.
- **Honest status.** If the Horizon side behaves differently, use `partial` and say how in `notes`. If there's no API, use `missing` and link the request (open one on Horizon if there isn't one).

## Where knowledge may come from

- Your own ports, the Horizon mod API and its docs, the Fabric API and its docs, and open-licensed mods whose license allows reading and reusing them.
- **Not** from mods that are all rights reserved, or whose license forbids it. Don't copy code from them into entries, notes or examples.
- Don't add Mojang's mapping files to the repo. Class and method names are fine.

## No AI

Hopper's tools are rule-based. Please don't add anything that calls an AI model.

## Code changes

The checker and generator aren't written yet. If you want to help with them, open an issue first so we can agree on the approach.

By contributing, you agree that your contribution is under the [MIT License](LICENSE).
