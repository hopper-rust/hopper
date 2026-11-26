//! Finished ports: Fabric mods someone ported to Horizon, tested in game and published on the
//! Horizon registry. One TOML file per port in `ports/` (see `ports/README.md`); Hopper points
//! to the finished mod instead of starting a new port.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Port {
    /// The file name without `.toml`.
    #[serde(default, skip_deserializing)]
    pub id: String,
    pub fabric: Fabric,
    pub horizon: Horizon,
    pub info: PortInfo,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Fabric {
    /// The Fabric mod's Modrinth slug.
    pub modrinth: String,
    /// The Fabric versions the port covers; empty means all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub versions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Horizon {
    /// The port's id on the Horizon registry (horizon-rust/horizon-mods).
    pub registry: String,
    /// The Horizon builds it works on, in words ("Nov 22+").
    pub builds: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortInfo {
    pub ported_by: String,
    /// When someone last tested it in game (YYYY-MM-DD).
    pub tested: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

#[derive(Debug, Default, Serialize)]
pub struct Ports {
    pub ports: Vec<Port>,
}

impl Ports {
    /// Reads every `*.toml` in `dir`; a missing folder is an empty list.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let mut ports = Vec::new();
        let Ok(items) = std::fs::read_dir(dir) else { return Ok(Self::default()) };
        for item in items {
            let path = item.map_err(|e| e.to_string())?.path();
            if path.extension().is_none_or(|e| e != "toml") {
                continue;
            }
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let mut port: Port = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            port.id = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            ports.push(port);
        }
        ports.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Self { ports })
    }

    /// The ports of the Fabric mod with this Modrinth slug.
    pub fn find(&self, slug: &str) -> Vec<&Port> {
        self.ports.iter().filter(|p| p.fabric.modrinth.eq_ignore_ascii_case(slug)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_port() {
        let port: Port = toml::from_str(
            r#"
[fabric]
modrinth = "showfps"
[horizon]
registry = "showfps"
builds = "Nov 22+"
[info]
ported_by = "someone"
tested = "2026-11-25"
"#,
        )
        .unwrap();
        let ports = Ports { ports: vec![port] };
        assert_eq!(ports.find("ShowFPS").len(), 1);
        assert!(ports.find("other").is_empty());
    }
}
