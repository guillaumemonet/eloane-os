//! `/etc/eloane/session.toml` configuration.

use std::path::Path;

use anyhow::Context;
use eloane_hw::GpuSelector;
use serde::{Deserialize, Serialize};

/// Default location of the optional configuration file.
pub const DEFAULT_CONFIG_PATH: &str = "/etc/eloane/session.toml";

/// Compositor selection in the configuration file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CompositorChoice {
    /// gamescope when a non-virtual render GPU exists, cage otherwise; falls
    /// back to cage once if gamescope keeps crashing.
    #[default]
    Auto,
    Gamescope,
    Cage,
}

impl CompositorChoice {
    pub fn name(self) -> &'static str {
        match self {
            CompositorChoice::Auto => "auto",
            CompositorChoice::Gamescope => "gamescope",
            CompositorChoice::Cage => "cage",
        }
    }
}

/// Session configuration. Every field is optional in the TOML file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Compositor: `auto` (default), `gamescope` or `cage`.
    pub compositor: CompositorChoice,
    /// Command run inside the compositor.
    pub command: Vec<String>,
    /// Extra gamescope arguments, inserted before `--` (ignored with cage).
    pub gamescope_args: Vec<String>,
    /// Output width (gamescope `-W`).
    pub width: Option<u32>,
    /// Output height (gamescope `-H`).
    pub height: Option<u32>,
    /// Render GPU override, `vendor:device` in hex (e.g. `"10de:28b8"`).
    pub prefer_gpu: Option<GpuSelector>,
    /// Maximum number of crashes tolerated within `restart_window_secs`.
    pub max_restarts: u32,
    /// Sliding window used to count crashes, in seconds.
    pub restart_window_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            compositor: CompositorChoice::Auto,
            command: [
                "steam",
                "-gamepadui",
                "-steamos3",
                "-steampal",
                "-steamdeck",
            ]
            .map(String::from)
            .to_vec(),
            gamescope_args: Vec::new(),
            width: None,
            height: None,
            prefer_gpu: None,
            max_restarts: 3,
            restart_window_secs: 60,
        }
    }
}

impl Config {
    /// Parses a TOML document.
    pub fn from_toml(s: &str) -> anyhow::Result<Self> {
        let config: Config = toml::from_str(s)?;
        anyhow::ensure!(!config.command.is_empty(), "`command` must not be empty");
        Ok(config)
    }

    /// Loads the configuration.
    ///
    /// With an explicit path the file must exist. Without one, the default
    /// path is used if present, otherwise built-in defaults apply.
    pub fn load(explicit: Option<&Path>) -> anyhow::Result<Self> {
        let (path, required) = match explicit {
            Some(p) => (p, true),
            None => (Path::new(DEFAULT_CONFIG_PATH), false),
        };
        match std::fs::read_to_string(path) {
            Ok(s) => {
                log::info!("loading config from {}", path.display());
                Self::from_toml(&s).with_context(|| format!("invalid config {}", path.display()))
            }
            Err(e) if !required && e.kind() == std::io::ErrorKind::NotFound => {
                log::debug!("no config at {}, using defaults", path.display());
                Ok(Config::default())
            }
            Err(e) => Err(e).with_context(|| format!("cannot read config {}", path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(Config::from_toml("").unwrap(), Config::default());
        assert_eq!(Config::default().command[0], "steam");
        assert_eq!(Config::default().max_restarts, 3);
        assert_eq!(Config::default().compositor, CompositorChoice::Auto);
        assert_eq!(Config::default().restart_window_secs, 60);
    }

    #[test]
    fn full_file() {
        let c = Config::from_toml(
            r#"
            compositor = "cage"
            command = ["steam", "-bigpicture"]
            gamescope_args = ["--adaptive-sync", "-r", "120"]
            width = 2560
            height = 1440
            prefer_gpu = "8086:a7a0"
            max_restarts = 5
            restart_window_secs = 30
            "#,
        )
        .unwrap();
        assert_eq!(c.compositor, CompositorChoice::Cage);
        assert_eq!(c.command, ["steam", "-bigpicture"]);
        assert_eq!(c.width, Some(2560));
        assert_eq!(c.prefer_gpu.unwrap().to_string(), "8086:a7a0");
        assert_eq!(c.max_restarts, 5);
    }

    #[test]
    fn rejects_bad_values() {
        assert!(Config::from_toml("prefer_gpu = \"nvidia\"").is_err());
        assert!(Config::from_toml("command = []").is_err());
        assert!(Config::from_toml("compositor = \"weston\"").is_err());
        assert!(Config::from_toml("unknown_key = 1").is_err());
    }

    #[test]
    fn missing_default_file_is_ok_but_missing_explicit_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.toml");
        assert!(Config::load(Some(&missing)).is_err());

        let file = dir.path().join("session.toml");
        std::fs::write(&file, "width = 1280\n").unwrap();
        assert_eq!(Config::load(Some(&file)).unwrap().width, Some(1280));
    }
}
