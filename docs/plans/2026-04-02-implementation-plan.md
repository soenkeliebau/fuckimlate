# fuckimlate Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a CLI tool that syncs Google Calendar events, extracts conference dial-in URLs, and joins meetings via fuzzel or auto-dial.

**Architecture:** Single Rust binary with 8 modules (`cli`, `config`, `extract`, `storage`, `calendar`, `handlers`, `ui`, plus top-level error). Blocking I/O with `reqwest::blocking`. SQLite for persistence. System keyring for OAuth tokens.

**Tech Stack:** Rust 2024 edition, clap, snafu, reqwest (blocking), rusqlite, chrono, regex, keyring, serde/toml, tracing

**Validation:** Every task ends with `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features && cargo build`

**Design doc:** `docs/plans/2026-04-02-architecture-design.md`
**Requirements:** `REQUIREMENTS.md`
**Coding guidelines:** `CLAUDE.md`

---

### Task 1: Project Scaffolding & Dependencies

**Files:**
- Modify: `Cargo.toml`
- Create: `src/cli.rs`
- Create: `src/config.rs`
- Create: `src/extract.rs`
- Create: `src/storage.rs`
- Create: `src/calendar.rs`
- Create: `src/handlers.rs`
- Create: `src/ui.rs`
- Modify: `src/main.rs`
- Create: `.gitignore`

**Step 1: Update Cargo.toml with all dependencies**

```toml
[package]
name = "fuckimlate"
version = "0.1.0"
edition = "2024"

[dependencies]
chrono = { version = "0.4", features = ["serde"] }
clap = { version = "4", features = ["derive"] }
dirs = "6"
keyring = { version = "3", features = ["linux-native"] }
regex = "1"
reqwest = { version = "0.12", features = ["blocking", "json"] }
rusqlite = { version = "0.32", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
snafu = "0.8"
toml = "0.8"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
url = "2"

[dev-dependencies]
tempfile = "3"
```

**Step 2: Create .gitignore**

```
/target
```

**Step 3: Create empty module files**

Each module file starts with just a comment placeholder. `src/main.rs` declares the modules:

```rust
mod calendar;
mod cli;
mod config;
mod extract;
mod handlers;
mod storage;
mod ui;

fn main() {
    println!("Hello, world!");
}
```

Each module file (`cli.rs`, `config.rs`, `extract.rs`, `storage.rs`, `calendar.rs`, `handlers.rs`, `ui.rs`) is empty.

**Step 4: Verify it compiles**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features && cargo build`
Expected: All pass, zero warnings.

**Step 5: Initialize git and commit**

```bash
git init
git add .gitignore Cargo.toml Cargo.lock src/ docs/ CLAUDE.md REQUIREMENTS.md
git commit -m "feat: project scaffolding with dependencies and module structure"
```

---

### Task 2: Data Model — ConferenceType & Meeting

**Files:**
- Create: `src/model.rs`
- Modify: `src/main.rs` (add `mod model`)

**Step 1: Write tests for ConferenceType**

In `src/model.rs`, write tests for `ConferenceType` Display, FromStr, and round-tripping:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn conference_type_display_round_trips() {
        let types = [
            ConferenceType::Zoom,
            ConferenceType::Teams,
            ConferenceType::GoogleMeet,
            ConferenceType::Slack,
            ConferenceType::WebEx,
            ConferenceType::Unknown,
        ];
        for ct in types {
            let s = ct.to_string();
            let parsed = ConferenceType::from_str(&s).expect("round-trip should work");
            assert_eq!(ct, parsed);
        }
    }

    #[test]
    fn conference_type_from_str_invalid() {
        let result = ConferenceType::from_str("nonsense");
        assert!(result.is_err());
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --all-features -- model::tests`
Expected: FAIL — `ConferenceType` not defined.

**Step 3: Implement ConferenceType and Meeting**

```rust
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Local};
use snafu::Snafu;

/// Error type for the model module.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
    /// An unknown conference type string was encountered.
    #[snafu(display("Unknown conference type: {value}"))]
    UnknownConferenceType { value: String },
}

/// Result type alias for the model module.
pub type Result<T> = std::result::Result<T, Error>;

/// The type of conference system a meeting uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConferenceType {
    /// Zoom meeting.
    Zoom,
    /// Microsoft Teams meeting.
    Teams,
    /// Google Meet meeting.
    GoogleMeet,
    /// Slack huddle.
    Slack,
    /// Cisco WebEx meeting.
    WebEx,
    /// Unknown or unrecognized conference system.
    Unknown,
}

impl fmt::Display for ConferenceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zoom => write!(f, "zoom"),
            Self::Teams => write!(f, "teams"),
            Self::GoogleMeet => write!(f, "meet"),
            Self::Slack => write!(f, "slack"),
            Self::WebEx => write!(f, "webex"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

impl FromStr for ConferenceType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "zoom" => Ok(Self::Zoom),
            "teams" => Ok(Self::Teams),
            "meet" => Ok(Self::GoogleMeet),
            "slack" => Ok(Self::Slack),
            "webex" => Ok(Self::WebEx),
            "unknown" => Ok(Self::Unknown),
            other => UnknownConferenceTypeSnafu {
                value: other.to_owned(),
            }
            .fail(),
        }
    }
}

/// A calendar meeting with optional conference dial-in information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meeting {
    /// Google Calendar event ID.
    pub id: String,
    /// Event title/summary.
    pub title: String,
    /// Event start time in local timezone.
    pub start_time: DateTime<Local>,
    /// Event end time in local timezone.
    pub end_time: DateTime<Local>,
    /// Extracted conference dial-in URL.
    pub conference_url: Option<String>,
    /// Detected conference system type.
    pub conference_type: Option<ConferenceType>,
    /// Raw location field from the calendar event.
    pub raw_location: Option<String>,
    /// Raw description field from the calendar event.
    pub raw_description: Option<String>,
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features && cargo build`
Expected: All pass.

**Step 5: Commit**

```bash
git add src/model.rs src/main.rs
git commit -m "feat: add Meeting and ConferenceType data model"
```

---

### Task 3: Config Module

**Files:**
- Modify: `src/config.rs`

**Step 1: Write tests for config defaults and loading**

```rust
#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(teams_handler.command, "google-chrome");
        assert!(teams_handler.args.iter().any(|a| a.contains("--app=")));
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
        // Overridden field
        assert_eq!(config.sync.stale_threshold_minutes, 10);
        // Default fields still present
        assert_eq!(config.panic_mode.lookback_minutes, 15);
        assert_eq!(config.ui.fuzzel_command, "fuzzel");
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --all-features -- config::tests`
Expected: FAIL — `Config` not defined.

**Step 3: Implement config structs and loading**

The config module defines:
- `Config` — top-level struct with `Default` impl
- `CalendarConfig` — `client_id`, `client_secret`, `calendar_ids`
- `SyncConfig` — `stale_threshold_minutes`, `storage_path`
- `PanicConfig` — `lookback_minutes`, `lookahead_minutes`
- `HandlerConfig` — `command`, `args`
- `UiConfig` — `fuzzel_command`, `fuzzel_args`
- `Config::from_toml_str(&str) -> Result<Config>` — parse TOML, merge with defaults
- `Config::load(path) -> Result<Config>` — read file, call `from_toml_str`
- `Config::load_default() -> Result<Config>` — find XDG config path, load or return defaults
- `Config::handler_for_type(ConferenceType) -> &HandlerConfig` — look up handler with fallback to default

All config structs derive `Debug, Clone, Serialize, Deserialize, Default` (with custom Default impls where needed).

Handlers are stored as `HashMap<String, HandlerConfig>` in the TOML structure. `handler_for_type` maps `ConferenceType::display()` to a key lookup, falling back to `"default"`.

The module has its own `Error` enum with variants: `ReadConfigFile { path, source: io::Error }`, `ParseConfig { source: toml::de::Error }`.

**Step 4: Run validation**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features && cargo build`
Expected: All pass.

**Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat: add config module with TOML loading and defaults"
```

---

### Task 4: Storage Module

**Files:**
- Modify: `src/storage.rs`

**Step 1: Write tests for storage operations**

Tests use `rusqlite::Connection::open_in_memory()` to avoid filesystem.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_meeting(id: &str, title: &str, hour: u32) -> Meeting {
        let start = Local.with_ymd_and_hms(2026, 4, 2, hour, 0, 0).unwrap();
        let end = Local.with_ymd_and_hms(2026, 4, 2, hour + 1, 0, 0).unwrap();
        Meeting {
            id: id.to_owned(),
            title: title.to_owned(),
            start_time: start,
            end_time: end,
            conference_url: Some("https://meet.google.com/abc".to_owned()),
            conference_type: Some(ConferenceType::GoogleMeet),
            raw_location: None,
            raw_description: None,
        }
    }

    #[test]
    fn store_and_retrieve_meetings() {
        let store = Storage::open_in_memory().expect("open in-memory db");
        let meetings = vec![make_meeting("1", "Standup", 9), make_meeting("2", "Lunch", 12)];
        store.replace_today(&meetings).expect("store meetings");
        let loaded = store.meetings_today().expect("load meetings");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].title, "Standup");
        assert_eq!(loaded[1].title, "Lunch");
    }

    #[test]
    fn replace_today_is_idempotent() {
        let store = Storage::open_in_memory().expect("open in-memory db");
        let meetings = vec![make_meeting("1", "Standup", 9)];
        store.replace_today(&meetings).expect("first store");
        store.replace_today(&meetings).expect("second store");
        let loaded = store.meetings_today().expect("load meetings");
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn staleness_check() {
        let store = Storage::open_in_memory().expect("open in-memory db");
        assert!(store.is_stale(60).expect("check staleness"), "should be stale with no sync");
        store.record_sync_time().expect("record sync");
        assert!(!store.is_stale(60).expect("check staleness"), "should not be stale right after sync");
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --all-features -- storage::tests`
Expected: FAIL — `Storage` not defined.

**Step 3: Implement Storage**

The storage module provides:
- `Storage::open(path: impl AsRef<Path>) -> Result<Storage>` — open/create SQLite DB at path, run migrations.
- `Storage::open_in_memory() -> Result<Storage>` — in-memory DB for tests.
- `Storage::replace_today(&self, meetings: &[Meeting]) -> Result<()>` — delete all meetings for today, insert new ones. Wrapped in a transaction.
- `Storage::meetings_today(&self) -> Result<Vec<Meeting>>` — query all meetings for today, sorted by `start_time`.
- `Storage::record_sync_time(&self) -> Result<()>` — upsert `last_sync_time` in `sync_meta`.
- `Storage::is_stale(&self, threshold_minutes: u64) -> Result<bool>` — check if `last_sync_time` is older than threshold.

Error variants: `OpenDatabase { path, source: rusqlite::Error }`, `Migration { source: rusqlite::Error }`, `InsertMeeting { id, source: rusqlite::Error }`, `QueryMeetings { source: rusqlite::Error }`, `RecordSyncTime { source: rusqlite::Error }`, `CheckStaleness { source: rusqlite::Error }`.

**Step 4: Run validation**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features && cargo build`
Expected: All pass.

**Step 5: Commit**

```bash
git add src/storage.rs
git commit -m "feat: add SQLite storage module with meeting persistence"
```

---

### Task 5: Extract Module — URL Pattern Matching

**Files:**
- Modify: `src/extract.rs`

**Step 1: Write tests for conference type detection from URLs**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_zoom_url() {
        let url = "https://us02web.zoom.us/j/12345678?pwd=abc";
        assert_eq!(detect_conference_type(url), Some(ConferenceType::Zoom));
    }

    #[test]
    fn detect_teams_url() {
        let url = "https://teams.microsoft.com/l/meetup-join/abc123";
        assert_eq!(detect_conference_type(url), Some(ConferenceType::Teams));
    }

    #[test]
    fn detect_google_meet_url() {
        let url = "https://meet.google.com/abc-defg-hij";
        assert_eq!(detect_conference_type(url), Some(ConferenceType::GoogleMeet));
    }

    #[test]
    fn detect_slack_url() {
        let url = "https://mycompany.slack.com/archives/C123/huddle";
        assert_eq!(detect_conference_type(url), Some(ConferenceType::Slack));
    }

    #[test]
    fn detect_webex_url() {
        let url = "https://mycompany.webex.com/meet/jdoe";
        assert_eq!(detect_conference_type(url), Some(ConferenceType::WebEx));
    }

    #[test]
    fn detect_unknown_url() {
        let url = "https://example.com/meeting/123";
        assert_eq!(detect_conference_type(url), Some(ConferenceType::Unknown));
    }

    #[test]
    fn extract_url_from_text_finds_zoom() {
        let text = "Join us at https://us02web.zoom.us/j/123 for the meeting. See you!";
        let (url, ct) = extract_conference_from_text(text).expect("should find URL");
        assert!(url.contains("zoom.us"));
        assert_eq!(ct, ConferenceType::Zoom);
    }

    #[test]
    fn extract_url_prefers_known_type_over_unknown() {
        let text = "Link: https://example.com/foo also https://meet.google.com/abc-def-ghi";
        let (url, ct) = extract_conference_from_text(text).expect("should find URL");
        assert_eq!(ct, ConferenceType::GoogleMeet);
        assert!(url.contains("meet.google.com"));
    }

    #[test]
    fn extract_url_from_text_no_urls() {
        let text = "No URLs here, just plain text about tomorrow's meeting.";
        assert!(extract_conference_from_text(text).is_none());
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --all-features -- extract::tests`
Expected: FAIL — functions not defined.

**Step 3: Implement extraction functions**

The extract module provides:
- `detect_conference_type(url: &str) -> Option<ConferenceType>` — match URL domain against known patterns. Returns `None` only if the input is not a valid URL. Returns `Some(Unknown)` for valid URLs with unrecognized domains.
- `extract_conference_from_text(text: &str) -> Option<(String, ConferenceType)>` — regex-find all URLs in text, return the first one with a known type. If none are known, return the first URL as `Unknown`.
- `extract_conference_info(conference_data: Option<&serde_json::Value>, location: Option<&str>, description: Option<&str>) -> Option<(String, ConferenceType)>` — the full priority chain:
  1. Check `conference_data` for video entry points.
  2. Check `location` via `extract_conference_from_text`.
  3. Check `description` via `extract_conference_from_text`.

Built-in patterns use domain matching via the `url` crate (parse URL, check host). No regex needed for domain matching itself — regex only for finding URLs in free text.

No module-level `Error` needed — these functions return `Option` since "no conference info found" is not an error.

**Step 4: Run validation**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features && cargo build`
Expected: All pass.

**Step 5: Commit**

```bash
git add src/extract.rs
git commit -m "feat: add conference URL extraction with type detection"
```

---

### Task 6: Handlers Module — Process Spawning

**Files:**
- Modify: `src/handlers.rs`

**Step 1: Write tests for URL template substitution**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitute_url_in_args() {
        let handler = HandlerConfig {
            command: "google-chrome".to_owned(),
            args: vec!["--app={url}".to_owned()],
        };
        let args = handler.build_args("https://meet.google.com/abc");
        assert_eq!(args, vec!["--app=https://meet.google.com/abc"]);
    }

    #[test]
    fn substitute_url_multiple_args() {
        let handler = HandlerConfig {
            command: "zoom".to_owned(),
            args: vec!["--url".to_owned(), "{url}".to_owned()],
        };
        let args = handler.build_args("https://zoom.us/j/123");
        assert_eq!(args, vec!["--url", "https://zoom.us/j/123"]);
    }

    #[test]
    fn resolve_handler_falls_back_to_default() {
        let config = Config::default();
        // ConferenceType::Unknown should use the default handler
        let handler = config.handler_for_type(ConferenceType::Unknown);
        assert_eq!(handler.command, "xdg-open");
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --all-features -- handlers::tests`
Expected: FAIL — `build_args` not defined.

**Step 3: Implement handlers**

The handlers module provides:
- `HandlerConfig::build_args(&self, url: &str) -> Vec<String>` — substitute `{url}` placeholder in each arg.
- `launch_meeting(meeting: &Meeting, config: &Config) -> Result<()>` — resolve handler for `meeting.conference_type`, substitute URL, spawn process detached. If `conference_url` is `None`, return error.
- `notify_error(message: &str) -> Result<()>` — call `notify-send` with the error message.

Process spawning: use `std::process::Command` with `.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())` and `.spawn()` (not `.status()`) so we don't block.

Error variants: `NoConferenceUrl { meeting_id: String, title: String }`, `SpawnHandler { command: String, source: std::io::Error }`, `SendNotification { source: std::io::Error }`.

**Step 4: Run validation**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features && cargo build`
Expected: All pass.

**Step 5: Commit**

```bash
git add src/handlers.rs
git commit -m "feat: add conference handlers with configurable command templates"
```

---

### Task 7: UI Module — Fuzzel Integration

**Files:**
- Modify: `src/ui.rs`

**Step 1: Write tests for meeting formatting**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_meeting(title: &str, hour: u32, has_url: bool) -> Meeting {
        let start = Local.with_ymd_and_hms(2026, 4, 2, hour, 0, 0).unwrap();
        let end = Local.with_ymd_and_hms(2026, 4, 2, hour + 1, 0, 0).unwrap();
        Meeting {
            id: "1".to_owned(),
            title: title.to_owned(),
            start_time: start,
            end_time: end,
            conference_url: if has_url { Some("https://meet.google.com/abc".to_owned()) } else { None },
            conference_type: if has_url { Some(ConferenceType::GoogleMeet) } else { None },
            raw_location: None,
            raw_description: None,
        }
    }

    #[test]
    fn format_meeting_with_url() {
        let m = make_meeting("Standup", 9, true);
        assert_eq!(format_meeting(&m), "[09:00] - Standup");
    }

    #[test]
    fn format_meeting_without_url() {
        let m = make_meeting("Lunch", 12, false);
        assert_eq!(format_meeting(&m), "[12:00] ? Lunch");
    }

    #[test]
    fn format_meeting_list() {
        let meetings = vec![
            make_meeting("Standup", 9, true),
            make_meeting("Lunch", 12, false),
        ];
        let lines = format_meeting_list(&meetings);
        assert_eq!(lines, "[09:00] - Standup\n[12:00] ? Lunch");
    }

    #[test]
    fn parse_selection_finds_meeting() {
        let meetings = vec![
            make_meeting("Standup", 9, true),
            make_meeting("Lunch", 12, false),
        ];
        let selected = parse_selection("[09:00] - Standup", &meetings);
        assert!(selected.is_some());
        assert_eq!(selected.expect("should find meeting").title, "Standup");
    }

    #[test]
    fn parse_selection_no_match() {
        let meetings = vec![make_meeting("Standup", 9, true)];
        let selected = parse_selection("garbage", &meetings);
        assert!(selected.is_none());
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --all-features -- ui::tests`
Expected: FAIL — functions not defined.

**Step 3: Implement UI functions**

The ui module provides:
- `format_meeting(meeting: &Meeting) -> String` — `[HH:MM] - Title` or `[HH:MM] ? Title`.
- `format_meeting_list(meetings: &[Meeting]) -> String` — newline-joined formatted meetings.
- `parse_selection<'a>(line: &str, meetings: &'a [Meeting]) -> Option<&'a Meeting>` — match the selected fuzzel line back to a meeting by comparing formatted output.
- `pick_meeting(meetings: &[Meeting], config: &UiConfig) -> Result<Option<&Meeting>>` — spawn fuzzel with `--dmenu`, pipe formatted list to stdin, read stdout, parse selection. Returns `None` if user dismissed fuzzel.

Error variants: `SpawnFuzzel { command: String, source: std::io::Error }`, `FuzzelIo { source: std::io::Error }`, `NoMeetingsToday`.

**Step 4: Run validation**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features && cargo build`
Expected: All pass.

**Step 5: Commit**

```bash
git add src/ui.rs
git commit -m "feat: add fuzzel UI module with meeting formatting and selection"
```

---

### Task 8: Calendar Module — OAuth2 & Google Calendar API

**Files:**
- Modify: `src/calendar.rs`

This is the hardest module to unit test because it talks to external services. Focus on testing the response parsing, not the HTTP calls.

**Step 1: Write tests for parsing Google Calendar API responses**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_event_with_conference_data() {
        let json = serde_json::json!({
            "id": "abc123",
            "summary": "Team Standup",
            "start": { "dateTime": "2026-04-02T09:00:00+02:00" },
            "end": { "dateTime": "2026-04-02T09:30:00+02:00" },
            "conferenceData": {
                "entryPoints": [{
                    "entryPointType": "video",
                    "uri": "https://meet.google.com/abc-def-ghi"
                }]
            }
        });
        let meeting = parse_event(&json).expect("should parse event");
        assert_eq!(meeting.id, "abc123");
        assert_eq!(meeting.title, "Team Standup");
        assert_eq!(meeting.conference_url.as_deref(), Some("https://meet.google.com/abc-def-ghi"));
        assert_eq!(meeting.conference_type, Some(ConferenceType::GoogleMeet));
    }

    #[test]
    fn parse_event_with_location_url() {
        let json = serde_json::json!({
            "id": "def456",
            "summary": "External Meeting",
            "start": { "dateTime": "2026-04-02T14:00:00+02:00" },
            "end": { "dateTime": "2026-04-02T15:00:00+02:00" },
            "location": "https://us02web.zoom.us/j/12345"
        });
        let meeting = parse_event(&json).expect("should parse event");
        assert_eq!(meeting.conference_type, Some(ConferenceType::Zoom));
    }

    #[test]
    fn parse_event_with_url_in_description() {
        let json = serde_json::json!({
            "id": "ghi789",
            "summary": "Vendor Call",
            "start": { "dateTime": "2026-04-02T16:00:00+02:00" },
            "end": { "dateTime": "2026-04-02T17:00:00+02:00" },
            "description": "Join here: https://teams.microsoft.com/l/meetup-join/abc"
        });
        let meeting = parse_event(&json).expect("should parse event");
        assert_eq!(meeting.conference_type, Some(ConferenceType::Teams));
    }

    #[test]
    fn parse_event_no_conference_info() {
        let json = serde_json::json!({
            "id": "jkl012",
            "summary": "1:1 Walk",
            "start": { "dateTime": "2026-04-02T11:00:00+02:00" },
            "end": { "dateTime": "2026-04-02T11:30:00+02:00" }
        });
        let meeting = parse_event(&json).expect("should parse event");
        assert!(meeting.conference_url.is_none());
        assert!(meeting.conference_type.is_none());
    }

    #[test]
    fn parse_event_all_day_returns_none() {
        let json = serde_json::json!({
            "id": "allday1",
            "summary": "Company Holiday",
            "start": { "date": "2026-04-02" },
            "end": { "date": "2026-04-03" }
        });
        let result = parse_event(&json);
        assert!(result.is_none(), "all-day events should be filtered out");
    }

    #[test]
    fn parse_events_list_response() {
        let json = serde_json::json!({
            "items": [
                {
                    "id": "1",
                    "summary": "Morning",
                    "start": { "dateTime": "2026-04-02T09:00:00+02:00" },
                    "end": { "dateTime": "2026-04-02T10:00:00+02:00" }
                },
                {
                    "id": "allday",
                    "summary": "Holiday",
                    "start": { "date": "2026-04-02" },
                    "end": { "date": "2026-04-03" }
                },
                {
                    "id": "2",
                    "summary": "Afternoon",
                    "start": { "dateTime": "2026-04-02T14:00:00+02:00" },
                    "end": { "dateTime": "2026-04-02T15:00:00+02:00" }
                }
            ]
        });
        let meetings = parse_events_list(&json).expect("should parse list");
        assert_eq!(meetings.len(), 2, "all-day event should be filtered");
        assert_eq!(meetings[0].title, "Morning");
        assert_eq!(meetings[1].title, "Afternoon");
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --all-features -- calendar::tests`
Expected: FAIL — functions not defined.

**Step 3: Implement calendar module**

The calendar module provides:
- `parse_event(json: &serde_json::Value) -> Option<Meeting>` — parse a single event JSON. Returns `None` for all-day events (no `dateTime`, only `date`). Uses `extract::extract_conference_info` for the priority chain.
- `parse_events_list(json: &serde_json::Value) -> Result<Vec<Meeting>>` — parse the `items` array from an `events.list` response, filtering out `None` (all-day events).
- `CalendarClient` struct holding `reqwest::blocking::Client`, base URL, and auth token.
- `CalendarClient::fetch_today(&self, calendar_id: &str) -> Result<Vec<Meeting>>` — call `events.list` with `timeMin`/`timeMax` for today, `singleEvents=true`, `orderBy=startTime`, `conferenceDataVersion=1`. Parse response.
- `CalendarClient::fetch_all_calendars(&self, calendar_ids: &[String]) -> Result<Vec<Meeting>>` — call `fetch_today` for each calendar ID, merge and sort by start time.
- OAuth2 functions:
  - `load_refresh_token() -> Result<Option<String>>` — try keyring.
  - `save_refresh_token(token: &str) -> Result<()>` — save to keyring.
  - `authenticate(config: &CalendarConfig) -> Result<String>` — full OAuth2 code flow: build auth URL, open in browser, listen on localhost, exchange code, save refresh token, return access token.
  - `get_access_token(config: &CalendarConfig) -> Result<String>` — load refresh token, exchange for access token. If fails, re-authenticate.

Error variants: `HttpRequest { url: String, source: reqwest::Error }`, `ApiError { status: u16, body: String }`, `ParseResponse { source: serde_json::Error }`, `ParseDateTime { value: String }`, `OAuth { reason: String }`, `Keyring { source: keyring::Error }`, `LocalServer { source: std::io::Error }`.

**Step 4: Run validation**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features && cargo build`
Expected: All pass.

**Step 5: Commit**

```bash
git add src/calendar.rs
git commit -m "feat: add Google Calendar API client with OAuth2 and event parsing"
```

---

### Task 9: CLI Module & Main Wiring

**Files:**
- Modify: `src/cli.rs`
- Modify: `src/main.rs`

**Step 1: Implement clap CLI structs**

```rust
use std::path::PathBuf;
use clap::{Parser, Subcommand};

/// Conference call dialer — never be late again.
#[derive(Debug, Parser)]
#[command(name = "fuckimlate", version, about)]
pub struct Cli {
    /// Path to config file.
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Increase log verbosity (-v, -vv, -vvv).
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Subcommand to run. Defaults to `pick`.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Available subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Fetch today's events from Google Calendar and update local storage.
    Sync,
    /// Show today's meetings in fuzzel and dial into the selected one.
    Pick,
    /// Auto-dial into the current or imminent meeting.
    Now,
    /// Print the resolved configuration.
    Config,
}
```

**Step 2: Wire up main.rs**

`main.rs` does:
1. Parse CLI args.
2. Initialize `tracing-subscriber` with verbosity from `-v` flags.
3. Load config (from `--config` path or XDG default).
4. Open storage (XDG data path).
5. Match on command:
   - `Sync` (or auto-sync): call `calendar::get_access_token`, `calendar::fetch_all_calendars`, `storage::replace_today`, `storage::record_sync_time`.
   - `Pick` (default): check staleness → maybe sync. Load meetings from storage. Call `ui::pick_meeting`. Call `handlers::launch_meeting`.
   - `Now`: check staleness → maybe sync. Load meetings. Filter by time window. If 1 match → launch. If multiple → `ui::pick_meeting` with subset. If none → notify.
   - `Config`: load and print config as TOML.
6. Top-level error handling: main returns `Result<(), Error>` where `Error` wraps all module errors. Print human-readable error to stderr on failure.

The top-level `Error` in `main.rs` (or a dedicated module) wraps each module's error:

```rust
#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Configuration error"))]
    Config { source: config::Error },
    #[snafu(display("Storage error"))]
    Storage { source: storage::Error },
    #[snafu(display("Calendar API error"))]
    Calendar { source: calendar::Error },
    #[snafu(display("Handler error"))]
    Handler { source: handlers::Error },
    #[snafu(display("UI error"))]
    Ui { source: ui::Error },
}
```

**Step 3: Run validation**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features && cargo build`
Expected: All pass.

**Step 4: Commit**

```bash
git add src/cli.rs src/main.rs
git commit -m "feat: wire up CLI with clap and connect all modules in main"
```

---

### Task 10: Integration Testing & Polish

**Files:**
- Create: `tests/integration.rs` (optional, for end-to-end CLI tests)
- Modify: any files that need fixes from integration testing

**Step 1: Manual smoke test of `sync` command**

Create a test config with real Google credentials, run `cargo run -- sync -v` and verify:
- OAuth flow opens browser
- Token is stored in keyring
- Meetings appear in SQLite DB

**Step 2: Manual smoke test of `pick` command**

Run `cargo run -- pick` and verify:
- fuzzel appears with today's meetings
- Selecting a meeting launches the right handler

**Step 3: Manual smoke test of `now` command**

Run `cargo run -- now` near a meeting time and verify:
- Auto-dials if one meeting matches
- Shows fuzzel if multiple
- Shows notification if none

**Step 4: Fix any issues found**

**Step 5: Final validation and commit**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features && cargo build`

```bash
git add -A
git commit -m "feat: integration testing and polish"
```
