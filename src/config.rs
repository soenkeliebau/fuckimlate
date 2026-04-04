use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};

use crate::model::ConferenceType;

/// Error type for the config module.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
    /// Failed to read a configuration file from disk.
    #[snafu(display("Failed to read config file at {path:?}"))]
    ReadConfigFile {
        /// The path that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },

    /// Failed to parse TOML configuration content.
    #[snafu(display("Failed to parse config TOML"))]
    ParseConfig {
        /// The underlying TOML deserialization error.
        source: toml::de::Error,
    },
}

/// Result type alias for the config module.
pub type Result<T> = std::result::Result<T, Error>;

/// Google Calendar API configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalendarConfig {
    /// OAuth2 client ID for Google Calendar API access.
    #[serde(default)]
    pub client_id: Option<String>,
    /// OAuth2 client secret for Google Calendar API access.
    #[serde(default)]
    pub client_secret: Option<String>,
    /// List of calendar IDs to synchronize.
    #[serde(default = "default_calendar_ids")]
    pub calendar_ids: Vec<String>,
}

/// Returns the default list of calendar IDs.
fn default_calendar_ids() -> Vec<String> {
    vec!["primary".to_owned()]
}

impl Default for CalendarConfig {
    fn default() -> Self {
        Self {
            client_id: None,
            client_secret: None,
            calendar_ids: default_calendar_ids(),
        }
    }
}

/// Configuration for calendar synchronization behavior.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncConfig {
    /// Number of minutes after which cached calendar data is considered stale.
    #[serde(default = "default_stale_threshold_minutes")]
    pub stale_threshold_minutes: u64,
    /// Optional path to the local storage file (SQLite database).
    #[serde(default)]
    pub storage_path: Option<PathBuf>,
}

/// Returns the default stale threshold in minutes.
fn default_stale_threshold_minutes() -> u64 {
    60
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            stale_threshold_minutes: default_stale_threshold_minutes(),
            storage_path: None,
        }
    }
}

/// Configuration for panic (auto-dial) mode timing windows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PanicConfig {
    /// How many minutes in the past to consider a meeting as "just started".
    #[serde(default = "default_lookback_minutes")]
    pub lookback_minutes: u64,
    /// How many minutes in the future to consider a meeting as "about to start".
    #[serde(default = "default_lookahead_minutes")]
    pub lookahead_minutes: u64,
}

/// Returns the default lookback window in minutes.
fn default_lookback_minutes() -> u64 {
    15
}

/// Returns the default lookahead window in minutes.
fn default_lookahead_minutes() -> u64 {
    5
}

impl Default for PanicConfig {
    fn default() -> Self {
        Self {
            lookback_minutes: default_lookback_minutes(),
            lookahead_minutes: default_lookahead_minutes(),
        }
    }
}

/// Configuration for a conference handler that launches a meeting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandlerConfig {
    /// The executable command to invoke.
    pub command: String,
    /// Arguments to pass to the command. Use `{url}` as a placeholder for the meeting URL.
    pub args: Vec<String>,
}

/// Configuration for the picker-based user interface.
///
/// The picker uses a fallback chain: fuzzel → rofi → terminal input.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UiConfig {
    /// Path or name of the fuzzel binary.
    #[serde(default = "default_fuzzel_command")]
    pub fuzzel_command: String,
    /// Arguments to pass to fuzzel.
    #[serde(default = "default_fuzzel_args")]
    pub fuzzel_args: Vec<String>,
    /// Path or name of the rofi binary.
    #[serde(default = "default_rofi_command")]
    pub rofi_command: String,
    /// Arguments to pass to rofi.
    #[serde(default = "default_rofi_args")]
    pub rofi_args: Vec<String>,
}

/// Returns the default fuzzel command name.
fn default_fuzzel_command() -> String {
    "fuzzel".to_owned()
}

/// Returns the default fuzzel arguments.
fn default_fuzzel_args() -> Vec<String> {
    vec![
        "--dmenu".to_owned(),
        "--width=60".to_owned(),
        "--icon-theme=Papirus".to_owned(),
    ]
}

/// Returns the default rofi command name.
fn default_rofi_command() -> String {
    "rofi".to_owned()
}

/// Returns the default rofi arguments.
fn default_rofi_args() -> Vec<String> {
    vec!["-dmenu".to_owned()]
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            fuzzel_command: default_fuzzel_command(),
            fuzzel_args: default_fuzzel_args(),
            rofi_command: default_rofi_command(),
            rofi_args: default_rofi_args(),
        }
    }
}

/// Top-level application configuration.
///
/// Loaded from a TOML file, with sane defaults for all fields.
/// The `[panic]` section in the TOML file maps to the `panic_mode` field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    /// Google Calendar API settings.
    #[serde(default)]
    pub calendar: CalendarConfig,
    /// Calendar synchronization settings.
    #[serde(default)]
    pub sync: SyncConfig,
    /// Panic (auto-dial) mode timing configuration.
    #[serde(rename = "panic", default)]
    pub panic_mode: PanicConfig,
    /// Conference type handler overrides, keyed by type name (e.g. "zoom", "teams").
    #[serde(default = "default_handlers")]
    pub handlers: HashMap<String, HandlerConfig>,
    /// Fuzzel UI configuration.
    #[serde(default)]
    pub ui: UiConfig,
}

/// Returns the default set of conference handlers.
fn default_handlers() -> HashMap<String, HandlerConfig> {
    let mut handlers = HashMap::new();
    handlers.insert(
        "zoom".to_owned(),
        HandlerConfig {
            command: "xdg-open".to_owned(),
            args: vec!["{url}".to_owned()],
        },
    );
    handlers.insert(
        "teams".to_owned(),
        HandlerConfig {
            command: "google-chrome-stable".to_owned(),
            args: vec!["{url}".to_owned()],
        },
    );
    handlers.insert(
        "meet".to_owned(),
        HandlerConfig {
            command: "google-chrome-stable".to_owned(),
            args: vec!["{url}".to_owned()],
        },
    );
    handlers.insert(
        "slack".to_owned(),
        HandlerConfig {
            command: "xdg-open".to_owned(),
            args: vec!["{url}".to_owned()],
        },
    );
    handlers.insert(
        "webex".to_owned(),
        HandlerConfig {
            command: "xdg-open".to_owned(),
            args: vec!["{url}".to_owned()],
        },
    );
    handlers.insert(
        "default".to_owned(),
        HandlerConfig {
            command: "xdg-open".to_owned(),
            args: vec!["{url}".to_owned()],
        },
    );
    handlers
}

impl Default for Config {
    fn default() -> Self {
        Self {
            calendar: CalendarConfig::default(),
            sync: SyncConfig::default(),
            panic_mode: PanicConfig::default(),
            handlers: default_handlers(),
            ui: UiConfig::default(),
        }
    }
}

impl Config {
    /// Parses a TOML string into a [`Config`], merging with defaults for any missing fields.
    ///
    /// User-provided `[handlers]` entries are merged on top of the defaults so that
    /// specifying e.g. only `[handlers.zoom]` does not discard the built-in handlers
    /// for teams, meet, etc.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ParseConfig`] if the TOML content is invalid or does not match the
    /// expected schema.
    pub fn from_toml_str(toml_str: &str) -> Result<Self> {
        let mut config: Config = toml::from_str(toml_str).context(ParseConfigSnafu)?;
        // Merge user handlers on top of defaults so partial overrides don't
        // lose the built-in handler entries.
        let mut merged = default_handlers();
        for (key, value) in config.handlers {
            merged.insert(key, value);
        }
        config.handlers = merged;
        Ok(config)
    }

    /// Loads configuration from a file at the given path.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ReadConfigFile`] if the file cannot be read.
    /// Returns [`Error::ParseConfig`] if the file content is not valid TOML.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path).context(ReadConfigFileSnafu { path })?;
        Self::from_toml_str(&content)
    }

    /// Loads configuration from the default XDG config path.
    ///
    /// Looks for `fuckimlate/config.toml` inside the user's XDG config directory
    /// (typically `~/.config`). If the file does not exist, returns the default configuration.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ReadConfigFile`] if the file exists but cannot be read.
    /// Returns [`Error::ParseConfig`] if the file content is not valid TOML.
    pub fn load_default() -> Result<Self> {
        let config_path = dirs::config_dir().map(|d| d.join("fuckimlate").join("config.toml"));

        match config_path {
            Some(path) if path.exists() => Self::load(&path),
            _ => Ok(Self::default()),
        }
    }

    /// Returns the [`HandlerConfig`] for the given [`ConferenceType`].
    ///
    /// Looks up the handler by the conference type's string representation (e.g. `"zoom"`,
    /// `"teams"`). If no specific handler is configured, falls back to the `"default"` handler.
    /// If neither is present, returns a static fallback that uses `xdg-open`.
    pub fn handler_for_type(&self, conference_type: ConferenceType) -> &HandlerConfig {
        static FALLBACK: std::sync::LazyLock<HandlerConfig> =
            std::sync::LazyLock::new(|| HandlerConfig {
                command: "xdg-open".to_owned(),
                args: vec!["{url}".to_owned()],
            });

        let key = conference_type.to_string();
        self.handlers
            .get(&key)
            .or_else(|| self.handlers.get("default"))
            .unwrap_or(&FALLBACK)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ConferenceType;

    #[test]
    fn default_config_has_sane_defaults() {
        let config = Config::default();
        assert_eq!(config.sync.stale_threshold_minutes, 60);
        assert_eq!(config.panic_mode.lookback_minutes, 15);
        assert_eq!(config.panic_mode.lookahead_minutes, 5);
        assert_eq!(config.ui.fuzzel_command, "fuzzel");
    }

    #[test]
    fn default_handlers_teams_uses_chrome() {
        let config = Config::default();
        let teams_handler = config.handler_for_type(ConferenceType::Teams);
        assert_eq!(teams_handler.command, "google-chrome-stable");
        assert!(teams_handler.args.iter().any(|a| a.contains("{url}")));
    }

    #[test]
    fn default_handlers_zoom_uses_xdg_open() {
        let config = Config::default();
        let zoom_handler = config.handler_for_type(ConferenceType::Zoom);
        assert_eq!(zoom_handler.command, "xdg-open");
    }

    #[test]
    fn load_config_from_toml_string() {
        let toml_str = r#"
[calendar]
client_id = "test-id"
client_secret = "test-secret"
calendar_ids = ["primary", "work"]

[sync]
stale_threshold_minutes = 30

[handlers.zoom]
command = "zoom"
args = ["--url", "{url}"]
"#;
        let config = Config::from_toml_str(toml_str).expect("should parse");
        assert_eq!(config.calendar.client_id, Some("test-id".to_owned()));
        assert_eq!(config.calendar.calendar_ids, vec!["primary", "work"]);
        assert_eq!(config.sync.stale_threshold_minutes, 30);
        let zoom = config.handler_for_type(ConferenceType::Zoom);
        assert_eq!(zoom.command, "zoom");
    }

    #[test]
    fn partial_config_merges_with_defaults() {
        let toml_str = r#"
[sync]
stale_threshold_minutes = 10
"#;
        let config = Config::from_toml_str(toml_str).expect("should parse");
        assert_eq!(config.sync.stale_threshold_minutes, 10);
        assert_eq!(config.panic_mode.lookback_minutes, 15);
        assert_eq!(config.ui.fuzzel_command, "fuzzel");
    }

    #[test]
    fn partial_handlers_merged_with_defaults() {
        let toml_str = r#"
[handlers.zoom]
command = "zoom"
args = ["--url", "{url}"]
"#;
        let config = Config::from_toml_str(toml_str).expect("should parse");
        // User override applied
        let zoom = config.handler_for_type(ConferenceType::Zoom);
        assert_eq!(zoom.command, "zoom");
        // Default handler still present
        let teams = config.handler_for_type(ConferenceType::Teams);
        assert_eq!(teams.command, "google-chrome-stable");
        // Default fallback still present
        let unknown = config.handler_for_type(ConferenceType::Unknown);
        assert_eq!(unknown.command, "xdg-open");
    }
}
