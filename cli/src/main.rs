//! `hopper check <modrinth link | slug | file.jar>`: Hopper's checker on the command line.

use hopper_core::library::{Library, Status};
use hopper_core::{Overall, Report};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USER_AGENT: &str = concat!("hopper-rust/hopper/", env!("CARGO_PKG_VERSION"), " (github.com/hopper-rust/hopper)");
const API: &str = "https://api.modrinth.com/v2";

const USAGE: &str = "\
Usage: hopper check <modrinth link | slug | file.jar> [options]

Options:
  --license <SPDX>    the license of a local jar (required for a jar)
  --game <version>    check the newest Fabric file for this Minecraft version
  --library <dir>     the library folder (default: ./library, or the repo's)
  --json              print the report as JSON
  --all               list every unknown class, not just the first 15";

struct Args {
    target: String,
    license: Option<String>,
    game: Option<String>,
    library: Option<PathBuf>,
    json: bool,
    all: bool,
}

fn args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    if it.next().as_deref() != Some("check") {
        return Err(USAGE.into());
    }
    let mut a = Args { target: String::new(), license: None, game: None, library: None, json: false, all: false };
    while let Some(arg) = it.next() {
        let mut value = || it.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--license" => a.license = Some(value()?),
            "--game" => a.game = Some(value()?),
            "--library" => a.library = Some(value()?.into()),
            "--json" => a.json = true,
            "--all" => a.all = true,
            s if s.starts_with("--") => return Err(format!("unknown option {s}\n\n{USAGE}")),
            s => a.target = s.to_string(),
        }
    }
    if a.target.is_empty() {
        return Err(USAGE.into());
    }
    Ok(a)
}

/// `./library`, or the repo's `library/` above the executable (for `cargo run`).
fn library_dir(given: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = given {
        return dir;
    }
    let here = PathBuf::from("library");
    if here.is_dir() {
        return here;
    }
    let exe = std::env::current_exe().unwrap_or_default();
    exe.ancestors().map(|d| d.join("library")).find(|d| d.is_dir()).unwrap_or(here)
}

fn get(url: &str) -> Result<ureq::Response, String> {
    ureq::get(url).set("User-Agent", USER_AGENT).call().map_err(|e| match e {
        ureq::Error::Status(404, _) => "not found on Modrinth".to_string(),
        e => format!("{url}: {e}"),
    })
}

fn json(url: &str) -> Result<serde_json::Value, String> {
    let body = get(url)?.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| e.to_string())
}

/// `https://modrinth.com/mod/sodium/versions` or `sodium`: the project's slug or id.
fn slug(target: &str) -> &str {
    let t = target.trim_end_matches('/');
    match t.split_once("modrinth.com/") {
        Some((_, rest)) => rest.split('/').nth(1).unwrap_or(rest),
        None => t,
    }
}

struct Fetched {
    title: String,
    /// The jar's file name, when one was downloaded.
    file: Option<String>,
}

/// The project's license, and its newest Fabric jar when the license allows porting.
fn fetch(library: &Library, target: &str, game: Option<&str>) -> Result<(Fetched, Report), String> {
    let slug = slug(target);
    let project = json(&format!("{API}/project/{slug}"))?;
    if project["project_type"].as_str() != Some("mod") {
        return Err(format!("{slug} is a {}, not a mod", project["project_type"].as_str().unwrap_or("project")));
    }
    let title = project["title"].as_str().unwrap_or(slug).to_string();
    let license = project["license"]["id"].as_str().unwrap_or_default().to_string();
    if let Ok(report) = hopper_core::check(library, &license, None) {
        // Stopped at the license: nothing is downloaded.
        return Ok((Fetched { title, file: None }, report));
    }
    let mut url = format!("{API}/project/{slug}/version?loaders=%5B%22fabric%22%5D");
    if let Some(g) = game {
        url += &format!("&game_versions=%5B%22{g}%22%5D");
    }
    let versions = json(&url)?;
    let version = versions.as_array().and_then(|v| v.first()).ok_or("no Fabric version on Modrinth")?;
    let files = version["files"].as_array().ok_or("the version has no files")?;
    let file = files.iter().find(|f| f["primary"].as_bool() == Some(true)).or(files.first()).ok_or("the version has no files")?;
    let (name, url) = (file["filename"].as_str().unwrap_or("mod.jar"), file["url"].as_str().ok_or("no file url")?);
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut get(url)?.into_reader(), &mut bytes).map_err(|e| e.to_string())?;
    let report = hopper_core::check(library, &license, Some(&bytes)).map_err(|e| e.0)?;
    Ok((Fetched { title, file: Some(name.to_string()) }, report))
}

fn print(title: &str, file: Option<&str>, report: &Report, all: bool) {
    println!("{title}");
    if let Some(f) = file {
        println!("  file: {f}");
    }
    println!();
    let l = &report.license;
    let id = if l.id.is_empty() { "none" } else { &l.id };
    println!("License: {id} ({:?})", l.verdict);
    println!("  {}", l.reason);
    if let Some(s) = &report.scan {
        println!();
        println!("Size: {} classes, {} mixins, side: {}", s.classes, s.mixins, s.environment);
        if !s.bundled.is_empty() {
            println!("  bundles (not checked): {}", s.bundled.join(", "));
        }
        if s.obfuscated {
            println!("  obfuscated");
        }
        println!();
        println!("Maps to Horizon:");
        if s.mapped.is_empty() {
            println!("  nothing yet");
        }
        for m in &s.mapped {
            let tag = match m.status {
                Status::Direct => "direct ",
                Status::Partial => "partial",
                Status::Missing => "MISSING",
            };
            println!("  [{tag}] {} -> {}{}", m.fabric, m.horizon, if m.via == "mixin" { " (mixin)" } else { "" });
            if m.status != Status::Direct {
                println!("            {}", m.notes);
            }
        }
        let shown = if all { s.unknown.len() } else { 15.min(s.unknown.len()) };
        println!();
        println!("Not in the library yet: {} classes", s.unknown.len());
        for u in &s.unknown[..shown] {
            let members = if u.members.is_empty() { "used as a type".to_string() } else { u.members.join(", ") };
            println!("  {} ({members})", u.class);
        }
        if shown < s.unknown.len() {
            println!("  ... and {} more (--all lists them)", s.unknown.len() - shown);
        }
        if !s.unknown_mixins.is_empty() {
            println!();
            println!("Mixin targets not in the library yet:");
            for m in &s.unknown_mixins {
                println!("  {m}");
            }
        }
    }
    println!();
    let verdict = match report.verdict {
        Overall::Easy => "easy",
        Overall::Partial => "partial",
        Overall::NeedsAPerson => "needs a person",
        Overall::CantPort => "can't port",
    };
    println!("Verdict: {verdict}");
    println!("  {}", report.summary);
}

fn run() -> Result<(), String> {
    let a = args()?;
    let dir = library_dir(a.library);
    let library = Library::load(&dir)?;
    let local = Path::new(&a.target);
    let (title, file, report) = if a.target.ends_with(".jar") && local.is_file() {
        let license = a.license.ok_or("a local jar needs --license <SPDX id>")?;
        let jar = std::fs::read(local).map_err(|e| format!("{}: {e}", local.display()))?;
        let report = hopper_core::check(&library, &license, Some(&jar)).map_err(|e| e.0)?;
        (a.target.clone(), None, report)
    } else {
        let (f, report) = fetch(&library, &a.target, a.game.as_deref())?;
        (f.title, f.file, report)
    };
    if a.json {
        println!("{}", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
    } else {
        print(&title, file.as_deref(), &report, a.all);
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
