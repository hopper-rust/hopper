# The library

One TOML file per mapping. The folder says what kind of thing it maps:

| Folder | What goes there |
| --- | --- |
| `events/` | Fabric API events → Horizon hooks |
| `mixins/` | common mixin targets → Horizon hooks |
| `keys/` | key bindings |
| `config/` | config screens (Cloth Config, YACL, ModMenu) → Horizon settings |
| `draw/` | drawing and other client calls → Horizon host functions |
| `types/` | basic Minecraft types (text, ids, positions) → mod API types or plain Rust |
| `gui/` | buttons, text boxes and other screen widgets |
| `missing/` | things Horizon has no API for yet |

Name the file after the Fabric side, in kebab case: `events/hud-render-callback.toml`.

## Entry format

```toml
# What the mod uses, on the Fabric side.
[from]
loader = "fabric"
class = "net.fabricmc.fabric.api.client.rendering.v1.HudRenderCallback"
member = "EVENT"            # field or method; leave out for the whole class
# For Minecraft classes, the Yarn names older mods use (optional), member after #.
# aliases = ["net.minecraft.client.gui.DrawContext#fill"]

# What it becomes on Horizon. Exactly one of: hook, host, api, rust, missing.
[to]
hook = "render_gui_post"
# host = "draw.fill"
# missing = "https://github.com/horizon-rust/horizon/issues/..."   # the API request, or "unrequested"

[info]
status = "direct"           # direct | partial | missing
notes = "Draw with event.draw; draw.size() is the GUI size."
since_api = "0.2"           # the Horizon mod API version this was checked on
seen_in = ["https://github.com/selfmonoraque5/skyblock-hud"]   # a tested port that does it
```

### Fields

- `from.loader`: `fabric` for now.
- `from.class`: the fully qualified class name. For Minecraft classes, use the **Mojang name** (mods for 26.1 and later ship with these). Put the older Yarn names in `aliases`, as `Class` or `Class#member`.
- `from.member`: the field or method, without arguments. Leave it out when the whole class maps.
- `to`: exactly one of
  - `hook`: a Horizon hook name, as in the mod API's `hooks/` folder.
  - `host`: a Horizon host function or draw call, like `draw.fill` or `host.send_chat`.
  - `api`: a mod API type or function that isn't a host call, like `mod_api::Text::literal`.
  - `rust`: plain Rust, when nothing from Horizon is needed, like `f32::clamp`.
  - `missing`: a link to the request for that API, or `"unrequested"` when nobody has asked yet. Use this when Horizon has nothing for it.
- `info.status`:
  - `direct`: same idea, a port only changes names.
  - `partial`: it maps, but something works differently. Say what in `notes`.
  - `missing`: no Horizon API. `to.missing` must be set.
- `info.notes`: what a porter needs to know, in a sentence or two.
- `info.since_api`: the mod API version you checked it on.
- `info.seen_in`: optional, links to tested ports that use this mapping.

Entries are checked against [`schema/entry.schema.json`](../schema/entry.schema.json) on every push and pull request. Run the same check locally with `pip install jsonschema` and `python scripts/check_library.py`.
