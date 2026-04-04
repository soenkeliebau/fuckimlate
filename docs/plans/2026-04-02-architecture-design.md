# fuckimlate — Architecture Design

## Overview

Single monolithic Rust binary for extracting conference call dial-in info from Google Calendar and joining meetings via fuzzel or auto-dial.

## Architecture

Single binary, 8 internal modules. Blocking I/O throughout (no async runtime).

### Module Structure

```
src/
  main.rs        — CLI entry point (clap), dispatches to commands
  cli.rs         — Clap derive structs
  config.rs      — TOML config loading, defaults, XDG paths
  calendar.rs    — Google Calendar REST client, OAuth2 auth
  extract.rs     — Conference URL extraction pipeline
  storage.rs     — SQLite operations
  handlers.rs    — Conference type detection + process spawning
  ui.rs          — fuzzel interaction
```

Each module defines its own `Error` enum + `Result` type alias using `snafu`.

### Data Flow

- **sync**: `calendar` → `extract` → `storage`
- **pick**: `storage` (staleness check → auto-sync) → `ui` (fuzzel) → `handlers` (spawn)
- **now**: `storage` (staleness check → auto-sync) → time filter → `handlers` or `ui` if ambiguous

## Data Model

```rust
pub enum ConferenceType {
    Zoom,
    Teams,
    GoogleMeet,
    Slack,
    WebEx,
    Unknown,
}

pub struct Meeting {
    pub id: String,
    pub title: String,
    pub start_time: DateTime<Local>,
    pub end_time: DateTime<Local>,
    pub conference_url: Option<String>,
    pub conference_type: Option<ConferenceType>,
    pub raw_location: Option<String>,
    pub raw_description: Option<String>,
}
```

### SQLite Schema

```sql
CREATE TABLE IF NOT EXISTS meetings (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    start_time TEXT NOT NULL,  -- RFC3339
    end_time TEXT NOT NULL,    -- RFC3339
    conference_url TEXT,
    conference_type TEXT,
    raw_location TEXT,
    raw_description TEXT
);

CREATE TABLE IF NOT EXISTS sync_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

Sync strategy: full replace for today's events (delete + insert). Idempotent. `sync_meta` tracks `last_sync_time` for staleness checks.

## Extraction Pipeline

Priority chain for dial-in URL extraction:

1. **Structured conference data** — `conferenceData.entryPoints[]` where `entryPointType == "video"`. Use the `uri`.
2. **Location field** — Regex match for conference URLs.
3. **Description field** — Regex match, take first recognized URL.

### ConferenceType Detection (from URL domain)

| Domain pattern | Type |
|---|---|
| `*.zoom.us` | Zoom |
| `teams.microsoft.com` | Teams |
| `meet.google.com` | GoogleMeet |
| `*.slack.com` | Slack |
| `*.webex.com` | WebEx |
| anything else | Unknown |

Built-in patterns always present. Users can add custom patterns in config (prepended, higher priority). Multiple URLs in a single field: first known-type match wins, else first URL as Unknown.

## Handlers

Configurable command templates per conference type. `{url}` placeholder substituted at runtime. Process spawned detached.

### Default Handlers

| Type | Command                | Args |
|---|------------------------|---|
| Zoom | `xdg-open`             | `["{url}"]` |
| Teams | `google-chrome-stable` | `["--app={url}"]` |
| GoogleMeet | `google-chrome-stable` | `["--app={url}"]` |
| Slack | `xdg-open`             | `["{url}"]` |
| WebEx | `xdg-open`             | `["{url}"]` |
| Default | `xdg-open`             | `["{url}"]` |

Teams and Meet default to Chrome because they work poorly in Firefox. Users override per handler in config.

No conference URL → error notification via `notify-send`. Command not found → error notification with failed command.

## UI (fuzzel)

- Format: `[HH:MM] - <Title>` per meeting, sorted by start time.
- No conference URL: `[HH:MM] ? <Title>` prefix.
- Spawn `fuzzel --dmenu`, pipe list to stdin, read selection from stdout.
- Parse selection back to identify meeting.
- Escape/no selection → exit cleanly.
- `now` with multiple matches → fuzzel with only matching meetings.
- No meetings → notification "No meetings today", exit 0.
- fuzzel not found → error notification + stderr.

## OAuth2 & Auth

- User provides `client_id` + `client_secret` in config (from their own Google Cloud project).
- Refresh token stored in system keyring via `secret-service` D-Bus API.
- Initial auth: OAuth2 authorization code flow → open consent URL via `xdg-open` → local redirect listener on `127.0.0.1` → exchange code → store refresh token.
- Subsequent runs: load refresh token from keyring → get fresh access token.
- Revoked/expired refresh token → re-trigger full auth flow.

### API Client

Hand-rolled thin client using `reqwest::blocking`. Only endpoint needed: `events.list` for a single day. No `google-calendar3` crate, no async runtime.

## Configuration

TOML at `~/.config/fuckimlate/config.toml` (respects `$XDG_CONFIG_HOME`).

```toml
[calendar]
client_id = "xxxxx.apps.googleusercontent.com"
client_secret = "GOCSPX-xxxxx"
calendar_ids = ["primary"]

[sync]
stale_threshold_minutes = 60

[panic]
lookback_minutes = 15
lookahead_minutes = 5

[handlers.zoom]
command = "xdg-open"
args = ["{url}"]

[handlers.teams]
command = "google-chrome"
args = ["--app={url}"]

[handlers.meet]
command = "google-chrome"
args = ["--app={url}"]

[handlers.default]
command = "xdg-open"
args = ["{url}"]

[ui]
fuzzel_command = "fuzzel"
fuzzel_args = ["--dmenu"]
```

## CLI Interface

```
fuckimlate [COMMAND]

Commands:
  sync     Fetch today's events from Google Calendar and update local storage
  pick     Show today's meetings in fuzzel and dial into the selected one (default)
  now      Auto-dial into the current/imminent meeting
  config   Print the resolved configuration
  help     Print help

Options:
  -c, --config <PATH>   Path to config file
  -v, --verbose         Increase log verbosity
  -h, --help            Print help
  -V, --version         Print version
```

## Auto-sync

`pick` and `now` check `sync_meta.last_sync_time`. If older than `stale_threshold_minutes` (default 60), trigger a sync before proceeding.

## Core Crates

| Purpose | Crate |
|---------|-------|
| HTTP client | `reqwest` (blocking) |
| SQLite | `rusqlite` |
| CLI parsing | `clap` (derive) |
| Config | `toml` + `serde` |
| Error handling | `snafu` |
| DateTime | `chrono` |
| URL patterns | `regex` + `url` |
| Keyring | `keyring` or `secret-service` |
| Logging | `tracing` |
| XDG dirs | `dirs` or `xdg` |
| Notifications | `notify-rust` or `std::process::Command` with `notify-send` |

## Out of Scope

- Proactive meeting notifications (left to calendar app).
- Multiple Google accounts (single account, multiple calendars only).
- All-day events (filtered out during sync).
