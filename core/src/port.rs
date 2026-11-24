//! Porting a mod straight from Modrinth: check it, convert what its source allows, and generate
//! the project. Shared by the command line and the website.

use crate::convert::{self, JavaFile};
use crate::generate::{self, Project, Source};
use crate::library::Library;
use crate::modrinth::{self, Checked};

/// The source zip is at most this big.
const MAX_SOURCE: u64 = 30 << 20;

pub struct Ported {
    pub checked: Checked,
    pub project: Project,
    /// Why the source wasn't converted, when it wasn't.
    pub source_note: Option<String>,
}

pub fn port(library: &Library, target: &str, game: Option<&str>, max_jar: u64) -> Result<Ported, String> {
    let f = modrinth::fetch(library, target, game, max_jar)?;
    let Some(jar) = &f.jar else {
        return Err(f.checked.report.summary.clone());
    };
    let project = modrinth::Project {
        slug: f.checked.slug.clone(),
        title: f.checked.title.clone(),
        icon_url: f.checked.icon_url.clone(),
        license: f.license.clone(),
        source_url: f.source_url.clone(),
    };
    let (project, source_note) = generate_for(library, &project, jar)?;
    Ok(Ported { checked: f.checked, project, source_note })
}

/// The project for a downloaded jar: converts what the mod's public source allows (only called
/// once the license allowed the jar to be downloaded), then generates. Also gives why nothing
/// was converted, when nothing was.
pub fn generate_for(library: &Library, p: &modrinth::Project, jar: &[u8]) -> Result<(Project, Option<String>), String> {
    let url = format!("https://modrinth.com/mod/{}", p.slug);
    let (converted, source_note) = match p.source_url.as_deref() {
        None => (None, Some("The mod links no source code on Modrinth, so nothing was converted.".to_string())),
        Some(src) => match modrinth::java_sources(src, MAX_SOURCE) {
            Ok(files) => {
                let files: Vec<JavaFile> = files.into_iter().map(|(path, text)| JavaFile { path, text }).collect();
                (Some((convert::convert(library, &files, &generate::key_id), src.to_string())), None)
            }
            Err(e) => (None, Some(format!("Nothing was converted: {e}."))),
        },
    };
    let source = Source {
        title: &p.title,
        url: &url,
        license: &p.license,
        converted: converted.as_ref().map(|(c, src)| (c, src.as_str())),
    };
    let project = generate::generate(library, &source, jar)?;
    Ok((project, source_note))
}
