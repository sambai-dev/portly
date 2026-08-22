use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use serde::Deserialize;

/// Runtime configuration. Everything has a default: a missing file is a valid
/// config (zero-config thesis). Unknown keys are ignored so older binaries
/// keep working with newer files.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub interval_ms: u64,
    pub sort: crate::model::SortKey,
    pub ignore_ports: BTreeSet<u16>,
    pub labels: HashMap<u16, String>,
    pub log_files: HashMap<u16, PathBuf>,
    pub theme: Theme,
    pub docker: DockerConfig,
    pub health: HealthConfig,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DockerConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HealthConfig {
    pub enabled: bool,
    pub interval_ms: u64,
    pub timeout_ms: u64,
    /// Path probed on every listening TCP port when health is enabled.
    pub path: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            interval_ms: 500,
            sort: crate::model::SortKey::Port,
            ignore_ports: BTreeSet::new(),
            labels: HashMap::new(),
            log_files: HashMap::new(),
            theme: Theme::default(),
            docker: DockerConfig { enabled: true },
            health: HealthConfig {
                enabled: false,
                interval_ms: 2000,
                timeout_ms: 750,
                path: "/".to_string(),
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct ConfigFile {
    interval_ms: Option<u64>,
    sort: Option<String>,
    ignore_ports: Option<Vec<u16>>,
    theme: Option<String>,
    labels: Option<HashMap<String, String>>,
    log_files: Option<HashMap<String, String>>,
    docker: Option<DockerSection>,
    health: Option<HealthSection>,
}

#[derive(Debug, Deserialize)]
struct DockerSection {
    enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct HealthSection {
    enabled: Option<bool>,
    interval_ms: Option<u64>,
    timeout_ms: Option<u64>,
    path: Option<String>,
}

impl Config {
    pub fn load() -> Self {
        Self::load_from(default_path().as_deref())
    }

    pub fn load_from(path: Option<&std::path::Path>) -> Self {
        let mut cfg = Config::default();
        let Some(path) = path else {
            return cfg;
        };
        let Ok(raw) = std::fs::read_to_string(path) else {
            return cfg;
        };
        let Ok(file) = toml::from_str::<ConfigFile>(&raw) else {
            eprintln!("portly: ignoring malformed config at {}", path.display());
            return cfg;
        };
        cfg.apply(file);
        cfg
    }

    fn apply(&mut self, file: ConfigFile) {
        if let Some(ms) = file.interval_ms {
            self.interval_ms = ms.clamp(100, 60_000);
        }
        if let Some(sort) = file.sort.as_deref() {
            if let Some(s) = crate::model::SortKey::parse(sort) {
                self.sort = s;
            }
        }
        if let Some(ports) = file.ignore_ports {
            self.ignore_ports = ports.into_iter().collect();
        }
        if let Some(name) = file.theme.as_deref() {
            self.theme = Theme::by_name(name);
        }
        if let Some(map) = file.labels {
            self.labels = map
                .into_iter()
                .filter_map(|(k, v)| k.parse::<u16>().ok().map(|p| (p, v)))
                .collect();
        }
        if let Some(map) = file.log_files {
            self.log_files = map
                .into_iter()
                .filter_map(|(k, v)| k.parse::<u16>().ok().map(|p| (p, PathBuf::from(v))))
                .collect();
        }
        if let Some(d) = file.docker {
            self.docker.enabled = d.enabled.unwrap_or(true);
        }
        if let Some(h) = file.health {
            self.health.enabled = h.enabled.unwrap_or(false);
            if let Some(v) = h.interval_ms {
                self.health.interval_ms = v.clamp(250, 3_600_000);
            }
            if let Some(v) = h.timeout_ms {
                self.health.timeout_ms = v.clamp(50, 10_000);
            }
            if let Some(p) = h.path {
                self.health.path = p;
            }
        }
    }

    #[cfg_attr(not(feature = "docker"), allow(dead_code))]
    pub fn ignored(&self, port: u16) -> bool {
        self.ignore_ports.contains(&port)
    }
}

fn default_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("PORTLY_CONFIG") {
        return Some(PathBuf::from(p));
    }
    dirs::config_dir().map(|d| d.join("portly").join("config.toml"))
}

// ---------------------------------------------------------------- themes ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub name: &'static str,
    pub border: ratatui::style::Color,
    pub header: ratatui::style::Color,
    pub accent: ratatui::style::Color,
    pub ok: ratatui::style::Color,
    pub warn: ratatui::style::Color,
    pub crit: ratatui::style::Color,
    pub muted: ratatui::style::Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

impl Theme {
    pub fn dark() -> Self {
        use ratatui::style::Color::*;
        Self {
            name: "dark",
            border: DarkGray,
            header: White,
            accent: Cyan,
            ok: Green,
            warn: Yellow,
            crit: Red,
            muted: DarkGray,
        }
    }

    pub fn light() -> Self {
        use ratatui::style::Color::*;
        Self {
            name: "light",
            border: Black,
            header: Black,
            accent: Blue,
            ok: Green,
            warn: LightYellow,
            crit: Red,
            muted: Gray,
        }
    }

    pub fn nord() -> Self {
        use ratatui::style::Color::*;
        Self {
            name: "nord",
            border: Indexed(59),  // nord3 polar night
            header: Indexed(216), // nord15 aurora
            accent: Indexed(109), // nord8 frost
            ok: Indexed(113),     // nord14
            warn: Indexed(179),   // nord13
            crit: Indexed(173),   // nord11
            muted: Indexed(60),   // nord9 dim frost
        }
    }

    pub fn by_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "light" => Self::light(),
            "nord" => Self::nord(),
            _ => Self::dark(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults() {
        let cfg = Config::load_from(None);
        assert_eq!(cfg, Config::default());
        assert!(!cfg.health.enabled);
        assert!(cfg.docker.enabled);
    }

    #[test]
    fn full_file_parses_and_clamps() {
        let dir = std::env::temp_dir().join("portly-test-config.toml");
        std::fs::write(
            &dir,
            r#"
interval_ms = 1
sort = "cpu"
ignore_ports = [19000, 19001]
theme = "nord"
labels = { "3000" = "vite dev" }
log_files = { "8080" = "/tmp/api.log" }
[docker]
enabled = false
[health]
enabled = true
interval_ms = 5
timeout_ms = 99_999
path = "/healthz"
"#,
        )
        .unwrap();
        let cfg = Config::load_from(Some(&dir));
        assert_eq!(cfg.interval_ms, 100, "clamped to minimum");
        assert_eq!(cfg.sort, crate::model::SortKey::Cpu);
        assert!(cfg.ignored(19000));
        assert_eq!(cfg.labels.get(&3000).map(String::as_str), Some("vite dev"));
        assert_eq!(
            cfg.log_files.get(&8080),
            Some(&PathBuf::from("/tmp/api.log"))
        );
        assert_eq!(cfg.theme.name, "nord");
        assert!(!cfg.docker.enabled);
        assert!(cfg.health.enabled);
        assert_eq!(cfg.health.interval_ms, 250, "clamped to minimum");
        assert_eq!(cfg.health.timeout_ms, 10_000, "clamped to maximum");
        assert_eq!(cfg.health.path, "/healthz");
        let _ = std::fs::remove_file(dir);
    }

    #[test]
    fn malformed_config_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join("portly-bad-config.toml");
        std::fs::write(&dir, "this is not [ valid toml ===").unwrap();
        assert_eq!(Config::load_from(Some(&dir)), Config::default());
        let _ = std::fs::remove_file(dir);
    }

    #[test]
    fn unknown_theme_name_falls_back_to_dark() {
        assert_eq!(Theme::by_name("solarized-eclipse"), Theme::dark());
        assert_eq!(Theme::by_name("LIGHT"), Theme::light());
    }
}

#[cfg(test)]
mod audit3_tests {
    use super::*;

    #[test]
    fn directory_as_config_path_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join("portly-config-dir-test");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(Config::load_from(Some(&dir)), Config::default());
        let _ = std::fs::remove_dir(dir);
    }
}
