"""Refreshes core/data/builds.toml, the Horizon builds a starter project can target, from the
SDK sources the Horizon launcher keeps (`%APPDATA%/horizon-launcher/native/source/<commit>`,
with the build's facts in `.../native/sdk/<commit>/sdk.json`).

    python scripts/update_builds.py <launcher native folder> [commit ...]

Without commits, every build in the folder is read. Horizon's API only grows from build to
build, so each build is written as the newest one minus what it lacks: the hooks (from
core/data/hooks.txt) and host functions (the ones Hopper's library and converter write) whose
`fn` isn't in that build's `horizon-mod-vocab` yet. core/data/hooks.txt itself comes from the
newest build (scripts/update_hooks.py)."""
import json
import pathlib
import re
import sys

root = pathlib.Path(__file__).resolve().parent.parent
native = pathlib.Path(sys.argv[1])
commits = sys.argv[2:] or sorted(p.name for p in (native / "source").iterdir() if p.is_dir())

hooks = [
    line.split("\t")[0]
    for line in (root / "core" / "data" / "hooks.txt").read_text(encoding="utf-8").splitlines()
    if line and not line.startswith("#")
]
# Host functions Hopper writes: `host.x` in the library and the converter, `draw.x` too.
used = set()
for path in [*(root / "library").rglob("*.toml"), *(root / "core" / "src").rglob("*.rs")]:
    used.update(re.findall(r"\b(?:host|draw)\.([a-z_][a-z0-9_]*)\b", path.read_text(encoding="utf-8")))
names = sorted(set(hooks) | used)

builds = []
for commit in commits:
    facts = json.loads((native / "sdk" / commit / "sdk.json").read_text(encoding="utf-8"))
    vocab = native / "source" / commit / "crates" / "horizon-mod-vocab" / "src"
    defined = set()
    for path in vocab.rglob("*.rs"):
        defined.update(re.findall(r"\bfn\s+([a-z_][a-z0-9_]*)", path.read_text(encoding="utf-8")))
    builds.append({
        "horizon": facts["build"]["commit"],
        "date": facts["build"]["date"],
        "game": facts["build"]["game"],
        "mixins": facts["mixins"]["rev"],
        "has": defined,
    })

builds.sort(key=lambda b: (b["date"], len(b["has"])))
newest = builds[-1]["has"]
lines = [
    "# Horizon builds a starter project can target, oldest first; written by scripts/update_builds.py.",
    "# `lacks`: hooks and host functions Hopper writes that this build doesn't have yet.",
    "# The last build is the newest: core/data/hooks.txt and sdk.toml come from it.",
]
for b in builds:
    lacks = [n for n in names if n in newest and n not in b["has"]]
    lines += ["", "[[build]]"]
    lines += [f'{k} = "{b[k]}"' for k in ("horizon", "date", "game", "mixins")]
    lines.append("lacks = [" + ", ".join(f'"{n}"' for n in lacks) + "]")
(root / "core" / "data" / "builds.toml").write_text("\n".join(lines) + "\n", encoding="utf-8")
print(f"{len(builds)} builds, newest {builds[-1]['horizon'][:7]} ({builds[-1]['date']})")
