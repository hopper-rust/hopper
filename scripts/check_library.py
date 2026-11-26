"""Checks every library entry: valid TOML, matches the schema, a kebab-case
file name, a hook the Horizon SDK has, and no two entries mapping the same
Fabric thing the same way. Also checks the finished ports in ports/."""
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

# Finished ports: one per registry id, matching its file name.
port_validator = Draft202012Validator(json.loads((root / "schema" / "port.schema.json").read_text(encoding="utf-8")))
ports = sorted((root / "ports").glob("*.toml"))
for path in ports:
    name = path.relative_to(root).as_posix()
    try:
        port = tomllib.loads(path.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as e:
        errors.append(f"{name}: not valid TOML: {e}")
        continue
    for err in port_validator.iter_errors(port):
        where = "/".join(str(p) for p in err.absolute_path) or "(top)"
        errors.append(f"{name}: {where}: {err.message}")
    registry = port.get("horizon", {}).get("registry")
    if registry and path.stem != registry:
        errors.append(f"{name}: name the file after its registry id ({registry}.toml)")

for e in errors:
    print(e)
print(f"{len(files)} entries, {len(ports)} ports, {len(errors)} problems")
sys.exit(1 if errors else 0)
