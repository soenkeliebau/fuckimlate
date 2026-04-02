# fuckimlate - Project Requirements

## Overview

**fuckimlate** is a CLI tool that extracts conference call dial-in information from Google Calendar and makes it as easy as possible to join meetings. It integrates with [fuzzel](https://codeberg.org/dnkl/fuzzel) for interactive meeting selection and supports auto-dialing into imminent meetings.

## Architecture

The system has two operational modes:

1. **Sync mode** (`fuckimlate sync`) - Runs periodically (via systemd timer or cron) to fetch today's calendar events and persist them locally.
2. **Interactive mode** (`fuckimlate` / `fuckimlate pick`) - Presents today's meetings via fuzzel and dials into the selected one.
3. **Panic mode** (`fuckimlate now`) - Automatically identifies and dials into the meeting that just started or is about to start, no interaction needed.

## Functional Requirements

### FR-1: Google Calendar Sync

- **FR-1.1**: Authenticate with Google Calendar API (OAuth2).
- **FR-1.2**: Fetch all calendar events for the current day (midnight to midnight, local timezone). All-day events are excluded.
- **FR-1.3**: Extract meeting dial-in information from events using the following priority:
  1. **Conference/dial-in data** - Use structured conference data from the Calendar API (e.g., `conferenceData.entryPoints`).
  2. **Location field** - Check for conference URLs.
  3. **Description field** - Fall back to regex pattern matching for URLs (Zoom links, Teams links, Meet links, etc.).
- **FR-1.4**: Persist extracted meetings to local storage (SQLite or flat file).
- **FR-1.5**: Sync should be idempotent - running it multiple times produces the same result. Updated/deleted events should be reflected.
- **FR-1.6**: `pick` and `now` commands auto-trigger a sync if local data is older than a configurable threshold (default: 60 minutes).

### FR-2: Meeting Data Model

Each persisted meeting must contain:

| Field | Type | Description |
|-------|------|-------------|
| `id` | String | Google Calendar event ID (unique key) |
| `title` | String | Event summary/title |
| `start_time` | DateTime | Event start time (local timezone) |
| `end_time` | DateTime | Event end time (local timezone) |
| `conference_url` | Option\<String\> | Extracted conference/dial-in URL |
| `conference_type` | Option\<Enum\> | Detected type: Zoom, Teams, Meet, Slack, WebEx, Unknown |
| `raw_location` | Option\<String\> | Original location field |
| `raw_description` | Option\<String\> | Original description field (for re-extraction if patterns improve) |

### FR-3: Interactive Meeting Selection (fuzzel)

- **FR-3.1**: Launch fuzzel with today's meetings formatted as: `[HH:MM] - <Title>`
- **FR-3.2**: Meetings should be sorted by start time.
- **FR-3.3**: Meetings without a detected conference URL should still be listed but visually differentiated (e.g., prefixed with a marker or dimmed).
- **FR-3.4**: When the user selects a meeting, dial into it using the appropriate handler (see FR-5).
- **FR-3.5**: If the selected meeting has no conference URL, show an error notification (via `notify-send` or similar).

### FR-4: Auto-Dial ("Panic Mode")

- **FR-4.1**: When invoked with `fuckimlate now`, find the most relevant meeting to join:
  1. A meeting currently in progress (started within the last N minutes, configurable, default 15).
  2. A meeting starting within the next N minutes (configurable, default 5).
- **FR-4.2**: If exactly one meeting matches, dial in immediately.
- **FR-4.3**: If multiple meetings match, fall back to fuzzel selection with only the matching meetings.
- **FR-4.4**: If no meeting matches, show a notification saying there's nothing to join.

### FR-5: Conference Handlers

The tool must support dialing into meetings using different applications depending on the conference type:

| Conference Type | Handler |
|-----------------|---------|
| Zoom | Launch `zoom` executable with the meeting URL |
| Microsoft Teams | Open URL in browser (configurable, default: Chrome) |
| Google Meet | Open URL in browser |
| Slack Huddle | Open URL via `slack` or `xdg-open` |
| WebEx | Open URL in browser |
| Unknown/Other | Open URL via `xdg-open` |

- **FR-5.1**: Handlers must be configurable - users should be able to override which application handles which conference type.
- **FR-5.2**: Each handler should support a configurable command template (e.g., `zoom --url {url}` or `google-chrome --app={url}`).

### FR-6: URL Pattern Detection

The tool must recognize at least these URL patterns in event descriptions:

| Service | Pattern Examples |
|---------|-----------------|
| Zoom | `https://*.zoom.us/j/...`, `https://*.zoom.us/my/...` |
| Teams | `https://teams.microsoft.com/l/meetup-join/...` |
| Google Meet | `https://meet.google.com/...` |
| Slack | `https://*.slack.com/...huddle...` |
| WebEx | `https://*.webex.com/...` |

- **FR-6.1**: Pattern matching should be extensible via configuration for custom/corporate conferencing tools.

## Non-Functional Requirements

### NFR-1: Performance

- Sync should complete in under 5 seconds for a typical day (< 20 meetings).
- Interactive mode (fuzzel launch) should feel instant (< 200ms from invocation to fuzzel appearing).

### NFR-2: Reliability

- If sync fails (network issues, auth expired), it must not corrupt existing local data.
- If a handler fails to launch, show a clear error notification.

### NFR-3: Configuration

Configuration via a TOML file at `~/.config/fuckimlate/config.toml` (respecting `$XDG_CONFIG_HOME`):

```toml
[calendar]
# Google Calendar ID(s) to sync
calendar_ids = ["primary"]

[sync]
# Local storage path (default: $XDG_DATA_HOME/fuckimlate/meetings.db)
# storage_path = "/path/to/meetings.db"
# Auto-sync staleness threshold in minutes (pick/now trigger sync if data is older)
stale_threshold_minutes = 60

[panic]
# Minutes after start to still consider a meeting "just started"
lookback_minutes = 15
# Minutes before start to consider a meeting "about to start"
lookahead_minutes = 5

[handlers.zoom]
command = "zoom"
args = ["--url", "{url}"]

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
# fuzzel command and extra args
fuzzel_command = "fuzzel"
fuzzel_args = ["--dmenu"]
```

### NFR-4: Platform

- Linux only (Wayland-first, given fuzzel dependency).
- Rust 2024 edition.

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

## External Dependencies (Runtime)

- `fuzzel` - Wayland dmenu replacement for interactive selection
- `notify-send` (or `libnotify`) - Desktop notifications for errors
- A browser (for Teams/Meet/WebEx)
- `zoom` (optional, for Zoom meetings)
- `slack` (optional, for Slack huddles)

## Key Crate Candidates

| Purpose | Crate |
|---------|-------|
| Google Calendar API | `google-calendar3` (google-apis-rs) |
| OAuth2 | `yup-oauth2` (pairs with google-apis-rs) |
| HTTP client | `hyper` / `reqwest` |
| Async runtime | `tokio` |
| CLI parsing | `clap` (derive) |
| Config parsing | `toml` + `serde` |
| SQLite | `rusqlite` |
| DateTime | `chrono` or `time` |
| Error handling | `snafu` |
| URL parsing/regex | `url` + `regex` |
| Keyring / secrets | `keyring` or `secret-service` |
| Logging | `tracing` |
| Process spawning | `std::process::Command` |

## Design Decisions

1. **Single account, multiple calendars**: One OAuth login, but supports syncing from multiple calendars within that account (configured via `calendar_ids`).
2. **All-day events filtered out**: All-day events are excluded during sync — they're rarely dial-in meetings.
3. **Credential storage via system keyring**: OAuth tokens are stored using the D-Bus secret-service API (GNOME Keyring / KWallet) for security.
4. **Auto-sync when stale**: `pick` and `now` automatically trigger a sync if local data is older than a configurable threshold (default: 1 hour). Explicit `sync` is still available.
5. **Recurring events**: The Google Calendar API expands recurring events into individual instances when querying a time range — no special handling needed.

## Out of Scope

- **Proactive meeting notifications**: Left to the calendar app. This tool only shows notifications for errors (e.g., failed handler launch, no conference URL).
