## What this changes

<!-- One topic per PR. -->

## How I checked it

<!-- Library entries: where you checked each name (Fabric API source, javap on the 26.3 jar,
     core/data/hooks.txt / the SDK). Converter rules: the test, and a port that compiles.
     Ports: the registry entry, and when you tested it in game. -->

## Checklist

- [ ] `python scripts/check_library.py` and `cargo test` pass, code is `cargo fmt`ed
- [ ] Every class, method and hook name is real and checked, not guessed
- [ ] Nothing comes from a mod whose license forbids it, and nothing is decompiled
- [ ] Nothing in Hopper calls an AI model
