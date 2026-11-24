"""Checks every library entry: valid TOML, matches the schema, a kebab-case
file name, a hook the Horizon SDK has, and no two entries mapping the same
Fabric thing the same way."""
import json
import pathlib
import re
import sys
import tomllib

from jsonschema import Draft202012Validator

root = pathlib.Path(__file__).resolve().parent.parent
validator = Draft202012Validator(json.loads((root / "schema" / "entry.schema.json").read_text(encoding="utf-8")))
hooks = {
    line.split("	")[0]
    for line in (root / "core" / "data" / "hooks.txt").read_text(encoding="utf-8").splitlines()
    if line and not line.startswith("#")
}
seen = {}
errors = []
files = sorted((root / "library").rglob("*.toml"))
for path in files:
    name = path.relative_to(root).as_posix()
    if not re.fullmatch(r"[a-z0-9]+(-[a-z0-9]+)*\.toml", path.name):
        errors.append(f"{name}: file names are kebab-case")
    try:
        entry = tomllib.loads(path.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as e:
        errors.append(f"{name}: not valid TOML: {e}")
        continue
    for err in validator.iter_errors(entry):
        where = "/".join(str(p) for p in err.absolute_path) or "(top)"
        errors.append(f"{name}: {where}: {err.message}")
    hook = entry.get("to", {}).get("hook")
    if hook and hook not in hooks:
        errors.append(f"{name}: no hook {hook} in core/data/hooks.txt (the Horizon SDK's hooks)")
    key = (entry.get("from", {}).get("class"), entry.get("from", {}).get("member"), json.dumps(entry.get("to"), sort_keys=True))
    if key in seen:
        errors.append(f"{name}: same mapping as {seen[key]}")
    seen[key] = name

for e in errors:
    print(e)
print(f"{len(files)} entries, {len(errors)} problems")
sys.exit(1 if errors else 0)
