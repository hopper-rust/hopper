<p align="center"><img src="assets/logo.png" alt="Hopper logo" width="128"></p>

# Hopper

Hopper helps port Fabric client mods (Java/Kotlin) to [Horizon](https://github.com/horizon-rust/horizon) native mods (Rust).

> **Hopper is not an official Horizon project.** It is made by mod authors, for mod authors, and is not endorsed by the Horizon team.

## What it does

Hopper keeps a **library** of mappings, each one saying "this Fabric thing is this Horizon thing":

| Fabric | Horizon |
| --- | --- |
| `HudRenderCallback.EVENT` | `render_gui_post` hook |
| `ClientTickEvents.END_CLIENT_TICK` | `client_tick_post` hook |
| `KeyBindingHelper.registerKeyBinding` | `register_key_mapping` |
| `DrawContext.fill` | `draw.fill` |
| drawing your own body in first person | no Horizon API yet |

On top of the library, Hopper will have:

1. **Checker:** paste a Modrinth link. Hopper checks the license, looks at what the mod uses, and says which parts map to Horizon and which don't: *easy*, *partial* or *needs a person*.
2. **Generator:** makes the Horizon mod project (`Cargo.toml`, `horizon-mod.toml`, hooks, settings, key binds, lang file, credits), with TODOs where the logic goes.
3. **Converter (later):** turns common Java patterns inside those hooks into Rust. It gets better as the library grows.

**Status:** early. The library format is set and the first entries are being added. The checker, generator and converter are not written yet.

## No AI inside

Everything in Hopper is rule-based: the library is plain files written by people, and the tools only match and fill in. Nothing is sent to an AI model, and the output never pretends to be more finished than it is.

## Rules

- **License first.** Hopper never helps port a mod that is all rights reserved, has no license, or is obfuscated. Mods under a copyleft license (GPL, LGPL, MPL...) are flagged: a port that reuses their code must keep that license.
- **Credit the original.** Every generated project credits the mod it came from, in `horizon-mod.toml` and the README.
- **Honest output.** Hopper always says what was not ported. A generated project with TODOs is a starting point, not a finished mod.
- **Test in game.** A ported mod must be tested in game before it goes to the Horizon mod registry.

## Contributing

The easiest way to help is adding a library entry: one small TOML file. See [CONTRIBUTING.md](CONTRIBUTING.md) and [library/README.md](library/README.md).

## License

[MIT](LICENSE).
