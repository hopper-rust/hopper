"""Refreshes core/data/hooks.txt, the hook signatures the generator writes, from a Horizon
SDK build: pass the `mod_api_generated.rs` a mod build leaves in
`target/<profile>/build/horizon-mod-vocab-*/out/`.

    python scripts/update_hooks.py path/to/mod_api_generated.rs <sdk commit>

Each line of the output is `name<TAB>signature`, the signature as the Hooks trait declares it."""
import pathlib
import re
import sys

source = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
commit = sys.argv[2] if len(sys.argv) > 2 else "unknown"
start = source.index("pub trait Hooks")
body = source[start:]
lines = []
for m in re.finditer(r"^fn ([a-z_0-9]+)(\(&self[^{]*?)\s*\{", body, re.M):
    name, rest = m.group(1), m.group(2)
    if name in ("subscribed", "listeners"):
        continue
    signature = " ".join(f"fn {name}{rest}".split())
    lines.append(f"{name}\t{signature}")
    # The trait ends where the next item starts.
out = pathlib.Path(__file__).resolve().parent.parent / "core" / "data" / "hooks.txt"
out.parent.mkdir(exist_ok=True)
header = f"# Hook signatures from Horizon SDK {commit}, written by scripts/update_hooks.py.\n"
out.write_text(header + "\n".join(sorted(set(lines))) + "\n", encoding="utf-8", newline="\n")
print(f"{len(set(lines))} hooks -> {out}")
