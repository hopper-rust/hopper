//! Hopper's checker: reads a Fabric mod's jar and says what of it maps to Horizon.
//!
//! [`check`] takes the mod's license and its jar's bytes. The license is checked first; the
//! jar is only read for a license that allows porting.

pub mod classfile;
pub mod library;
pub mod license;

use classfile::Ref;
use library::{Library, Status};
use license::{LicenseCheck, Verdict};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

/// Packages whose names the library maps. Anything else (the mod's own code, Java, Kotlin,
/// other libraries) is left out of the report.
const MAPPED_PACKAGES: &[&str] = &[
    "net.minecraft.",
    "com.mojang.",
    "net.fabricmc.fabric.api.",
    "me.shedaniel.clothconfig2.",
    "me.shedaniel.autoconfig.",
    "dev.isxander.yacl3.",
    "com.terraformersmc.modmenu.",
];

#[derive(Debug, Serialize)]
pub struct Report {
    pub license: LicenseCheck,
    /// Missing when the license stops the check: the jar isn't read.
    pub scan: Option<Scan>,
    pub verdict: Overall,
    /// Why the verdict is what it is, in a sentence.
    pub summary: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Overall {
    Easy,
    Partial,
    NeedsAPerson,
    CantPort,
}

#[derive(Debug, Default, Serialize)]
pub struct Scan {
    pub mod_id: String,
    pub name: String,
    pub version: String,
    /// `client`, `server` or `*` (both), from `fabric.mod.json`.
    pub environment: String,
    pub classes: usize,
    pub mixins: usize,
    /// Jars bundled inside (jar-in-jar), not read.
    pub bundled: Vec<String>,
    pub obfuscated: bool,
    /// One row per library entry the mod uses.
    pub mapped: Vec<Mapped>,
    /// Classes the mod uses that the library doesn't know yet, with how many of their
    /// members it uses, most used first.
    pub unknown: Vec<Unknown>,
    /// Mixin targets the library doesn't know: `Class#method`.
    pub unknown_mixins: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Mapped {
    pub entry: String,
    pub fabric: String,
    pub horizon: String,
    pub status: Status,
    pub notes: String,
    /// How it is used: `call` (a reference in the code) or `mixin`.
    pub via: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Unknown {
    pub class: String,
    pub members: Vec<String>,
}

#[derive(Debug)]
pub struct CheckError(pub String);

/// Checks a mod: `license` is its SPDX id (as Modrinth gives it), `jar` its Fabric jar. Pass no
/// jar to check only the license (Hopper downloads the jar only when the license allows it).
pub fn check(library: &Library, license: &str, jar: Option<&[u8]>) -> Result<Report, CheckError> {
    let license = license::check(license);
    if license.verdict == Verdict::Stop {
        return Ok(Report {
            summary: license.reason.clone(),
            license,
            scan: None,
            verdict: Overall::CantPort,
        });
    }
    let Some(jar) = jar else {
        return Err(CheckError("the license allows porting, but no jar was given".into()));
    };
    let scan = scan(library, jar)?;
    let (verdict, summary) = judge(&scan);
    Ok(Report { license, scan: Some(scan), verdict, summary })
}

fn mapped(class: &str) -> bool {
    MAPPED_PACKAGES.iter().any(|p| class.starts_with(p))
}

pub fn scan(library: &Library, jar: &[u8]) -> Result<Scan, CheckError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(jar)).map_err(|e| CheckError(format!("not a jar: {e}")))?;
    let mut scan = Scan::default();
    let mut refs: BTreeSet<Ref> = BTreeSet::new();
    let mut mixin_uses: BTreeSet<(String, String)> = BTreeSet::new();
    let mut own_names = Vec::new();
    let mut fabric_json = None;

    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| CheckError(e.to_string()))?;
        let name = file.name().to_string();
        if name == "fabric.mod.json" {
            let mut s = String::new();
            file.read_to_string(&mut s).map_err(|e| CheckError(format!("fabric.mod.json: {e}")))?;
            fabric_json = Some(s);
        } else if name.starts_with("META-INF/jars/") && name.ends_with(".jar") {
            scan.bundled.push(name.rsplit('/').next().unwrap_or(&name).to_string());
        } else if name.ends_with(".class") && !name.starts_with("META-INF/") {
            let mut data = Vec::with_capacity(file.size() as usize);
            file.read_to_end(&mut data).map_err(|e| CheckError(e.to_string()))?;
            let Ok(class) = classfile::parse(&data) else { continue };
            scan.classes += 1;
            own_names.push(class.name.clone());
            if let Some(m) = &class.mixin {
                scan.mixins += 1;
                for t in &m.targets {
                    if m.methods.is_empty() {
                        mixin_uses.insert((t.clone(), String::new()));
                    }
                    for method in &m.methods {
                        mixin_uses.insert((t.clone(), method.clone()));
                    }
                }
            }
            refs.extend(class.refs.into_iter().filter(|r| mapped(&r.class)));
        }
    }

    let Some(json) = fabric_json else {
        return Err(CheckError("no fabric.mod.json: this isn't a Fabric mod".into()));
    };
    // Fabric Loader accepts line breaks and tabs inside strings; strict JSON doesn't. Outside
    // strings they are whitespace either way, so spaces keep the file's meaning.
    let json: String = json.trim_start_matches('\u{feff}').chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let meta: serde_json::Value = serde_json::from_str(&json).map_err(|e| CheckError(format!("fabric.mod.json: {e}")))?;
    let text = |k: &str| meta.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
    scan.mod_id = text("id");
    scan.name = text("name");
    scan.version = text("version");
    scan.environment = meta.get("environment").and_then(|v| v.as_str()).unwrap_or("*").to_string();
    scan.obfuscated = obfuscated(&own_names);

    // Code references: one row per entry; a class reference alone counts only when no member of it matched.
    let mut rows: BTreeMap<String, Mapped> = BTreeMap::new();
    let mut unknown: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for r in &refs {
        let found = library.find(&r.class, r.member.as_deref());
        if found.is_empty() {
            let members = unknown.entry(r.class.clone()).or_default();
            if let Some(m) = &r.member {
                members.insert(m.clone());
            }
            continue;
        }
        for e in found {
            rows.entry(e.id.clone()).or_insert_with(|| row(e, r, "call"));
        }
    }
    // A class whose members all mapped, or that only appears as a type, isn't unknown on its own.
    unknown.retain(|class, members| !(members.is_empty() && rows.values().any(|m| m.fabric.starts_with(class.as_str()))));

    for (class, method) in &mixin_uses {
        let member = (!method.is_empty()).then_some(method.as_str());
        let found = library.find(class, member);
        if found.is_empty() {
            scan.unknown_mixins.push(if method.is_empty() { class.clone() } else { format!("{class}#{method}") });
        }
        for e in found {
            let r = Ref { class: class.clone(), member: member.map(str::to_string) };
            rows.entry(e.id.clone()).or_insert_with(|| row(e, &r, "mixin"));
        }
    }

    scan.mapped = rows.into_values().collect();
    scan.mapped.sort_by(|a, b| a.status.cmp(&b.status).then(a.entry.cmp(&b.entry)));
    let mut unknown: Vec<Unknown> =
        unknown.into_iter().map(|(class, members)| Unknown { class, members: members.into_iter().collect() }).collect();
    unknown.sort_by(|a, b| b.members.len().cmp(&a.members.len()).then(a.class.cmp(&b.class)));
    scan.unknown = unknown;
    Ok(scan)
}

fn row(e: &library::Entry, r: &Ref, via: &'static str) -> Mapped {
    let fabric = match &r.member {
        Some(m) => format!("{}#{m}", r.class),
        None => r.class.clone(),
    };
    Mapped { entry: e.id.clone(), fabric, horizon: e.target(), status: e.info.status, notes: e.info.notes.clone(), via }
}

/// Most of the mod's own classes named with one or two letters (`a`, `b`, `ab`): an obfuscator ran.
fn obfuscated(names: &[String]) -> bool {
    if names.len() < 10 {
        return false;
    }
    let short = names
        .iter()
        .filter(|n| {
            let simple = n.rsplit('.').next().unwrap_or(n);
            let simple = simple.split('$').next().unwrap_or(simple);
            simple.len() <= 2
        })
        .count();
    short * 2 > names.len()
}

fn judge(scan: &Scan) -> (Overall, String) {
    if scan.obfuscated {
        return (
            Overall::CantPort,
            "Most of this mod's classes have one- or two-letter names: it is obfuscated, so Hopper doesn't port it.".into(),
        );
    }
    let count = |s: Status| scan.mapped.iter().filter(|m| m.status == s).count();
    let (direct, partial, missing) = (count(Status::Direct), count(Status::Partial), count(Status::Missing));
    let unknown = scan.unknown.len() + scan.unknown_mixins.len();
    let known = direct + partial;
    let total = known + missing + unknown;
    let mut summary = format!(
        "{direct} direct, {partial} partial and {missing} missing mappings; {} classes and {} mixin targets the library doesn't know yet.",
        scan.unknown.len(),
        scan.unknown_mixins.len()
    );
    let verdict = if total == 0 {
        summary = "This mod uses nothing Hopper maps: it may not need Minecraft at all, or it only bundles other mods.".into();
        Overall::NeedsAPerson
    } else if missing > 0 {
        summary += " Something it does has no Horizon API yet.";
        Overall::NeedsAPerson
    } else if known * 10 >= total * 9 {
        Overall::Easy
    } else if known * 10 >= total * 6 {
        Overall::Partial
    } else {
        Overall::NeedsAPerson
    };
    (verdict, summary)
}
