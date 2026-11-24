//! The generator: a starting Horizon mod project for a Fabric mod the license lets Hopper port.
//!
//! It writes the manifest, the crate, a `Hooks` impl with one method per hook the library maps
//! the mod's Fabric code to (each with TODOs saying what went there), the key mappings its lang
//! file names, the original's lang file and license, and a README listing what wasn't ported.
//! It ports no logic: the project builds, but does nothing until the TODOs are done.

use crate::library::{Entry, Library, Status};
use crate::license::Verdict;
use crate::{Mapped, Scan};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{Cursor, Read};

const HOOKS: &str = include_str!("../data/hooks.txt");
const SDK: &str = include_str!("../data/sdk.toml");

/// Where the project's files come from, besides the jar.
pub struct Source<'a> {
    /// The mod's name and page, for credits.
    pub title: &'a str,
    pub url: &'a str,
    /// Its SPDX license id.
    pub license: &'a str,
}

/// The generated files: paths relative to the project folder, and their contents.
pub struct Project {
    pub id: String,
    pub files: Vec<(String, Vec<u8>)>,
}

struct Meta {
    id: String,
    name: String,
    description: String,
    authors: Vec<String>,
    environment: String,
}

fn hook_signatures() -> BTreeMap<&'static str, &'static str> {
    HOOKS.lines().filter(|l| !l.starts_with('#')).filter_map(|l| l.split_once('\t')).collect()
}

fn sdk_value(key: &str) -> &'static str {
    SDK.lines()
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == key)
        .map_or("", |(_, v)| v.trim().trim_matches('"'))
}

/// A Horizon mod id: `[a-z0-9_]{1,64}`.
fn horizon_id(fabric_id: &str) -> String {
    let id: String = fabric_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    let id = id.trim_matches('_').chars().take(64).collect::<String>();
    if id.is_empty() { "ported_mod".into() } else { id }
}

fn pascal(id: &str) -> String {
    let s: String = id
        .split('_')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
        })
        .collect();
    if s.starts_with(|c: char| c.is_ascii_digit()) { format!("Mod{s}") } else { s }
}

/// A TOML basic string.
fn toml_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// One line of a `//` comment: no line breaks.
fn comment_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn read_meta(json: &str) -> Meta {
    let json: String = json.trim_start_matches('\u{feff}').chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
    let text = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let authors = v
        .get("authors")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string).or_else(|| x.get("name")?.as_str().map(str::to_string)))
                .collect()
        })
        .unwrap_or_default();
    Meta {
        id: text("id"),
        name: text("name"),
        description: text("description"),
        authors,
        environment: v.get("environment").and_then(|x| x.as_str()).unwrap_or("*").to_string(),
    }
}

/// Generates the project. Refuses a license that stops porting, and an obfuscated mod.
pub fn generate(library: &Library, source: &Source, jar: &[u8]) -> Result<Project, String> {
    let license = crate::license::check(source.license);
    if license.verdict == Verdict::Stop {
        return Err(license.reason);
    }
    let scan = crate::scan(library, jar).map_err(|e| e.0)?;
    if scan.obfuscated {
        return Err("This mod is obfuscated, so Hopper doesn't port it.".into());
    }

    let mut zip = zip::ZipArchive::new(Cursor::new(jar)).map_err(|e| format!("not a jar: {e}"))?;
    let mut read = |name: &str| -> Option<Vec<u8>> {
        let mut f = zip.by_name(name).ok()?;
        let mut out = Vec::new();
        f.read_to_end(&mut out).ok()?;
        Some(out)
    };
    let meta = read_meta(&String::from_utf8_lossy(&read("fabric.mod.json").unwrap_or_default()));
    let lang = read(&format!("assets/{}/lang/en_us.json", meta.id));
    let license_text = ["LICENSE", "LICENSE.txt", "LICENSE.md", "COPYING", "COPYING.txt"]
        .iter()
        .find_map(|n| read(n))
        .or_else(|| read(&format!("LICENSE_{}", meta.id)));

    let id = horizon_id(&meta.id);
    let name = if meta.name.is_empty() { source.title.to_string() } else { meta.name.clone() };
    let side = match meta.environment.as_str() {
        "client" => "client",
        "server" => "server",
        _ => "both",
    };
    let by = if meta.authors.is_empty() { String::new() } else { format!(" by {}", meta.authors.join(", ")) };
    let credits = format!(
        "Port of {name}{by} ({}, {}), started with Hopper. Not affiliated with {name}.",
        license.id, source.url
    );

    // Lang keys of key mappings: `key.<mod>.<action>`, not the categories.
    let keys: Vec<String> = lang
        .as_deref()
        .and_then(|l| serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(l).ok())
        .map(|m| {
            m.keys()
                .filter(|k| k.starts_with("key.") && !k.starts_with("key.categories") && !k.starts_with("key.category"))
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    let entries: BTreeMap<&str, &Entry> = library.entries.iter().map(|e| (e.id.as_str(), e)).collect();
    let mut files = Vec::new();
    files.push(("Cargo.toml".to_string(), cargo_toml(&id, &name, &license.id).into_bytes()));
    files.push((
        "horizon-mod.toml".to_string(),
        manifest(&id, &name, &meta.description, &license.id, &credits, side).into_bytes(),
    ));
    files.push(("src/lib.rs".to_string(), lib_rs(&id, &name, source, &scan, &entries, &keys).into_bytes()));
    if let Some(lang) = lang {
        files.push((format!("assets/{id}/lang/en_us.json"), lang));
    }
    let has_license = license_text.is_some();
    if let Some(text) = license_text {
        files.push(("LICENSE".to_string(), text));
    }
    files.push(("README.md".to_string(), readme(&name, source, &license.id, &credits, &scan, has_license).into_bytes()));
    files.push((".gitignore".to_string(), b"/target\n/.horizon\n".to_vec()));
    Ok(Project { id, files })
}

fn cargo_toml(id: &str, name: &str, license: &str) -> String {
    format!(
        r#"[package]
name = "mod-{crate_name}"
version = "0.1.0"
edition = "2024"
license = {license}
publish = false
description = {description}

[dependencies]
# The pinned SDK's API crate, fetched into .horizon/sdk/ by any `cargo horizon-mod` command.
horizon-mod-api = {{ path = ".horizon/sdk/api/crates/horizon-mod-api" }}
"#,
        crate_name = id.replace('_', "-"),
        license = toml_str(license),
        description = toml_str(&format!("{name} for Horizon, started with Hopper.")),
    )
}

fn manifest(id: &str, name: &str, description: &str, license: &str, credits: &str, side: &str) -> String {
    format!(
        r#"id = {id}
name = {name}
version = "0.1.0"
description = {description}
# TODO: your name.
authors = []
license = {license}
credits = {credits}
api = "0.2"
side = {side}
capabilities = ["registries"]

# The Horizon build Hopper checked the library against. Pin the build you test on.
[native]
game = {game}
horizon = {horizon}
mixins = {mixins}
"#,
        id = toml_str(id),
        name = toml_str(name),
        description = toml_str(description),
        license = toml_str(license),
        credits = toml_str(credits),
        side = toml_str(side),
        game = toml_str(sdk_value("game")),
        horizon = toml_str(sdk_value("horizon")),
        mixins = toml_str(sdk_value("mixins")),
    )
}

fn status(s: Status) -> &'static str {
    match s {
        Status::Direct => "direct",
        Status::Partial => "partial",
        Status::Missing => "missing",
    }
}

/// The `use` list for the signatures written: every `CamelCase` type name in them.
fn imports(signatures: &[&str], extra: &[&str]) -> String {
    let mut names: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
    for sig in signatures {
        for word in sig.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
            if word.starts_with(|c: char| c.is_ascii_uppercase()) && word != "Self" {
                names.push(word.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    format!("use mod_api::{{self, {}}};", names.join(", "))
}

fn lib_rs(id: &str, name: &str, source: &Source, scan: &Scan, entries: &BTreeMap<&str, &Entry>, keys: &[String]) -> String {
    let sigs = hook_signatures();
    // Each hook, with what of the original lands there.
    let mut by_hook: BTreeMap<&str, Vec<&Mapped>> = BTreeMap::new();
    let mut elsewhere: Vec<&Mapped> = Vec::new();
    for m in &scan.mapped {
        match entries.get(m.entry.as_str()).and_then(|e| e.to.hook.as_deref()) {
            Some(h) if sigs.contains_key(h) && h != "register" => by_hook.entry(h).or_default().push(m),
            _ => elsewhere.push(m),
        }
    }
    let register = sigs.get("register").copied().unwrap_or("fn register(&self, r: &mut dyn Registrar, cx: &RegistrationContext)");
    let mut used: Vec<&str> = vec![register];
    used.extend(by_hook.keys().filter_map(|h| sigs.get(h).copied()));
    let extra: &[&str] = if keys.is_empty() { &["Hooks"] } else { &["Hooks", "KeyRegistration"] };

    let mut s = String::new();
    let _ = writeln!(s, "//! {name} for Horizon: a port of {} ({}), started with Hopper.", source.title, source.url);
    let _ = writeln!(s, "//!");
    let _ = writeln!(s, "//! Hopper wrote the hooks the original's Fabric code maps to, with a TODO in each");
    let _ = writeln!(s, "//! saying what the original did there. No logic is ported yet: see README.md for");
    let _ = writeln!(s, "//! what has no mapping, and test the mod in game before publishing it.");
    if !elsewhere.is_empty() {
        let _ = writeln!(s, "//!");
        let _ = writeln!(s, "//! Calls the original makes that become host calls or plain Rust:");
        for m in &elsewhere {
            let _ = writeln!(s, "//! - `{}` -> {} ({})", m.fabric, m.horizon, status(m.status));
        }
    }
    s.push('\n');
    let _ = writeln!(s, "{}", imports(&used, extra));
    s.push('\n');
    let _ = writeln!(s, "pub const ID: &str = {};", toml_str(id));
    s.push('\n');
    let ty = pascal(id);
    let _ = writeln!(s, "#[derive(Default)]\nstruct {ty};\n");
    let _ = writeln!(s, "#[mod_api::horizon_mod]\nimpl Hooks for {ty} {{");

    // register: the key mappings, and a TODO for settings.
    let config = scan.mapped.iter().any(|m| m.entry.starts_with("config/"));
    let reg = if keys.is_empty() { register.replace("r:", "_r:") } else { register.to_string() }.replace("cx:", "_cx:");
    let _ = writeln!(s, "    {reg} {{");
    for k in keys {
        // `key.<mod>.main.edit` becomes `main_edit`: unique, and a plain id.
        let rest = k.strip_prefix("key.").unwrap_or(k);
        let rest = rest.split_once('.').map_or(rest, |(_, r)| r);
        let action: String =
            rest.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
        let action = action.as_str();
        let _ = writeln!(
            s,
            "        // TODO: pick a default key (\"key.keyboard.k\"); read it with host.consume_click(ID, {}).",
            toml_str(action)
        );
        let _ = writeln!(s, "        r.register_key_mapping(KeyRegistration::new({}, \"key.keyboard.unknown\").label({}));", toml_str(action), toml_str(k));
    }
    if config {
        let _ = writeln!(s, "        // TODO: one SettingSpec per option of the original's config screen (r.register_setting).");
    }
    if keys.is_empty() && !config {
        let _ = writeln!(s, "        // Nothing to register: the original has no key mappings or config screen Hopper found.");
    }
    let _ = writeln!(s, "    }}");

    for (hook, rows) in &by_hook {
        let sig = sigs[hook];
        let sig = sig.replace("host:", "_host:").replace("event:", "_event:");
        s.push('\n');
        let _ = writeln!(s, "    {sig} {{");
        for m in rows {
            let via = if m.via == "mixin" { "a mixin on " } else { "" };
            let _ = writeln!(s, "        // TODO: port {via}`{}` ({}).", m.fabric, status(m.status));
            if m.status != Status::Direct {
                let _ = writeln!(s, "        //   {}", comment_line(&m.notes));
            }
        }
        if sig.ends_with("-> Flow") {
            let _ = writeln!(s, "        Flow::Continue");
        }
        let _ = writeln!(s, "    }}");
    }
    let _ = writeln!(s, "}}");
    s
}

fn readme(name: &str, source: &Source, license: &str, credits: &str, scan: &Scan, has_license: bool) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# {name} for Horizon\n");
    let _ = writeln!(s, "{credits}\n");
    let _ = writeln!(s, "> **Not finished.** Hopper made this project from [{}]({})'s Fabric jar: the hooks are in place, but no logic is ported yet. Do the TODOs in `src/lib.rs`, port what is listed below, and test the mod in game before publishing it.\n", source.title, source.url);
    let _ = writeln!(s, "## License\n");
    if has_license {
        let _ = writeln!(s, "{license}, as the original: see [LICENSE](LICENSE). Keep the original's copyright notice, and say what you changed.\n");
    } else {
        let _ = writeln!(s, "{license}, as the original. **TODO:** the jar had no license file; add the original's license text as `LICENSE`.\n");
    }
    let missing: Vec<&Mapped> = scan.mapped.iter().filter(|m| m.status == Status::Missing).collect();
    let _ = writeln!(s, "## Not ported\n");
    if missing.is_empty() && scan.unknown.is_empty() && scan.unknown_mixins.is_empty() {
        let _ = writeln!(s, "Hopper found a mapping for everything the original uses.\n");
    }
    if !missing.is_empty() {
        let _ = writeln!(s, "No Horizon API yet:\n");
        for m in &missing {
            let _ = writeln!(s, "- `{}`: {}", m.fabric, comment_line(&m.notes));
        }
        s.push('\n');
    }
    if !scan.unknown.is_empty() {
        let _ = writeln!(s, "Not in Hopper's library yet (find the Horizon way by hand, and consider adding a mapping):\n");
        for u in &scan.unknown {
            if u.members.is_empty() {
                let _ = writeln!(s, "- `{}`", u.class);
            } else {
                let _ = writeln!(s, "- `{}`: {}", u.class, u.members.join(", "));
            }
        }
        s.push('\n');
    }
    if !scan.unknown_mixins.is_empty() {
        let _ = writeln!(s, "Mixin targets not in the library yet:\n");
        for m in &scan.unknown_mixins {
            let _ = writeln!(s, "- `{m}`");
        }
        s.push('\n');
    }
    if !scan.bundled.is_empty() {
        let _ = writeln!(s, "Bundled jars Hopper didn't check: {}\n", scan.bundled.join(", "));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_names() {
        assert_eq!(horizon_id("command-keys"), "command_keys");
        assert_eq!(horizon_id("MyMod"), "mymod");
        assert_eq!(horizon_id("--"), "ported_mod");
        assert_eq!(pascal("command_keys"), "CommandKeys");
        assert_eq!(pascal("3d_skin"), "Mod3dSkin");
    }

    #[test]
    fn hook_data_is_there() {
        let sigs = hook_signatures();
        assert!(sigs["client_tick_post"].starts_with("fn client_tick_post(&self"));
        assert!(sigs.contains_key("register"));
        assert_eq!(sdk_value("game"), "26.3");
    }

    #[test]
    fn toml_strings_escape() {
        assert_eq!(toml_str("a \"b\" \\"), "\"a \\\"b\\\" \\\\\"");
    }
}
