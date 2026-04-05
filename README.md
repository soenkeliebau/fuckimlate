# fuckimlate
This tool was born out of one too many occurences of me receiving a slack message "are you coming to the meeting?" and me thinking "fuck, I'm late!" and frantically searching through my calendar looking for dial in details..

It extracts conference call dial-in information from Google Calendar and makes joining meetings as easy as possible. Uses a picker fallback chain for meeting selection: [fuzzel](https://codeberg.org/dnkl/fuzzel) (Wayland) → [rofi](https://github.com/davatorium/rofi) → plain terminal input — so it works everywhere from a tiling Wayland compositor to a bare SSH session.

Plus it also has a "panic" mode, which simply dials you into whatever meeting your calendar says you should be in right now ..

## Setup

### 1. Google Cloud Credentials

Create a Google Cloud project with the Calendar API enabled:

1. Go to [Google Cloud Console](https://console.cloud.google.com/)
2. Create a new project
3. Enable the **Google Calendar API**
4. Create **OAuth 2.0 Client ID** credentials (Desktop application type)
5. Note your `client_id` and `client_secret`

### 2. Configuration

Create `~/.config/fuckimlate/config.toml` (see [`config.toml.template`](config.toml.template) for a fully annotated example with all available options):

```toml
[calendar]
client_id = "YOUR_CLIENT_ID.apps.googleusercontent.com"
client_secret = "YOUR_CLIENT_SECRET"
calendar_ids = ["primary"]
```

Additional configuration sections (all optional, shown with defaults):

```toml
[sync]
stale_threshold_minutes = 60          # auto-sync threshold for pick/now
# storage_path = "/custom/path.db"   # default: $XDG_DATA_HOME/fuckimlate/meetings.db

[panic]
lookback_minutes = 15   # how far back to consider a meeting "just started"
lookahead_minutes = 5   # how far ahead to consider a meeting "about to start"

[ui]
fuzzel_command = "fuzzel"
fuzzel_args = ["--dmenu", "--width=60", "--icon-theme=Papirus"]
rofi_command = "rofi"
rofi_args = ["-dmenu"]

# Override conference handlers (use {url} as placeholder):
# [handlers.zoom]
# command = "zoom"
# args = ["--url", "{url}"]
```

### 3. First Run

Run `fuckimlate sync` to authenticate. A browser window will open for Google OAuth consent. After granting access, your refresh token is stored in the system keyring.

## Usage

```
fuckimlate              # Pick a meeting via fuzzel/rofi/terminal and dial in
fuckimlate sync         # Fetch today's events from Google Calendar
fuckimlate now          # Auto-dial into the current/imminent meeting
fuckimlate config       # Print resolved configuration
```

The `pick` command (default) shows today's meetings in a picker. It tries fuzzel first, then rofi, and falls back to a numbered terminal prompt if neither is installed.

### Global Options

| Flag | Description |
|------|-------------|
| `-c`, `--config <PATH>` | Use a custom config file instead of the default |
| `-v`, `-vv`, `-vvv` | Increase log verbosity (warn, info, debug, trace) |

### Typical Workflow

1. Set up a systemd timer to run `fuckimlate sync` periodically (e.g., every 30 minutes)
2. Bind `fuckimlate` to a keyboard shortcut in your compositor (e.g., Super+M)
3. When late to a meeting, run `fuckimlate now` and it dials in automatically

### Auto-sync

`pick` and `now` automatically trigger a sync if local data is older than the configured threshold (default: 60 minutes, configurable via `sync.stale_threshold_minutes`).

### The `now` Command

`now` finds meetings that are currently ongoing, recently started (within `lookback_minutes`), or about to start (within `lookahead_minutes`). When a current meeting is ending soon and the next one is about to start, it automatically selects the upcoming meeting — so you dial into the right call during back-to-back meetings.

## Default Handlers

| Conference Type | Command | Notes |
|----------------|---------|-------|
| Zoom | `xdg-open {url}` | Native client picks up the URL |
| Teams | `google-chrome-stable {url}` | Works better in Chromium |
| Google Meet | `google-chrome-stable {url}` | Works better in Chromium |
| Slack | `xdg-open {url}` | Slack client handles its URLs |
| WebEx | `xdg-open {url}` | |
| Other | `xdg-open {url}` | Fallback |

Override any handler in your config file under `[handlers.<type>]`.

## Icons

When using fuzzel, meeting entries display conference-type icons from your installed icon theme. The default fuzzel args include `--icon-theme=Papirus`.

| Conference Type | Icon name |
|----------------|---------------------|
| Zoom | `us.zoom.Zoom` |
| Teams | `teams-for-linux` |
| Google Meet | `google-meet` |
| Slack | `slack` |
| WebEx | `appointment-soon` |
| Unknown | `appointment-soon` |
| No conference | `appointment-soon` |

Icon themes like [Papirus](https://github.com/PapirusDevelopmentTeam/papirus-icon-theme) provide good coverage. If an icon is not found in your theme, fuzzel silently skips it.

## Runtime Dependencies

- `xdg-utils` (xdg-open) — used by most default handlers
- D-Bus + a secret service provider (GNOME Keyring, KWallet, etc.) — for OAuth token storage
- `fuzzel` (optional) — preferred meeting picker on Wayland
- `rofi` (optional) — used as picker when fuzzel is not available
- If neither fuzzel nor rofi is installed, a plain numbered terminal prompt is used
- `notify-send` (optional) — desktop notifications for errors; degrades gracefully if missing
- A Chromium-based browser (optional) — for Teams/Meet default handlers

## Building

### With Cargo

```sh
cargo build --release
```

### With Nix

```sh
nix build
```

Or run directly:

```sh
nix run github:soenkeliebau/fuckimlate
```

### NixOS / Home Manager

Add to your flake inputs and include the package:

```nix
{
  inputs.fuckimlate.url = "github:soenkeliebau/fuckimlate";

  # In your system or home-manager config:
  environment.systemPackages = [ inputs.fuckimlate.packages.${system}.default ];
}
```

A standalone derivation is also available at `nix/package.nix` for use with `fetchFromGitHub` (note: you will need to compute the `cargoHash` on first build).

## License

Apache-2.0
