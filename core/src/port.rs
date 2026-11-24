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
    let url = format!("https://modrinth.com/mod/{}", f.checked.slug);
    // The license already allows porting (the jar was downloaded); its source may be read too.
    let (converted, source_note) = match f.source_url.as_deref() {
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
        title: &f.checked.title,
        url: &url,
        license: &f.license,
        converted: converted.as_ref().map(|(c, src)| (c, src.as_str())),
    };
    let project = generate::generate(library, &source, jar)?;
    Ok(Ported { checked: f.checked, project, source_note })
}
