# fuckimlate

A CLI tool that extracts conference call dial-in information from Google Calendar and makes joining meetings as easy as possible. Integrates with [fuzzel](https://codeberg.org/dnkl/fuzzel) for interactive meeting selection on Wayland.

## Setup

### 1. Google Cloud Credentials

Create a Google Cloud project with the Calendar API enabled:

1. Go to [Google Cloud Console](https://console.cloud.google.com/)
2. Create a new project
3. Enable the **Google Calendar API**
4. Create **OAuth 2.0 Client ID** credentials (Desktop application type)
5. Note your `client_id` and `client_secret`

### 2. Configuration

Create `~/.config/fuckimlate/config.toml`:

```toml
[calendar]
client_id = "YOUR_CLIENT_ID.apps.googleusercontent.com"
client_secret = "YOUR_CLIENT_SECRET"
calendar_ids = ["primary"]

# Optional: customize conference handlers
# [handlers.teams]
# command = "google-chrome"
# args = ["--app={url}"]

# [handlers.zoom]
# command = "zoom"
# args = ["--url", "{url}"]
```

### 3. First Run

Run `fuckimlate sync` to authenticate. A browser window will open for Google OAuth consent. After granting access, your refresh token is stored in the system keyring.

## Usage

```
fuckimlate              # Pick a meeting from today's calendar via fuzzel
fuckimlate sync         # Fetch today's events from Google Calendar
fuckimlate now          # Auto-dial into the current/imminent meeting
fuckimlate config       # Print resolved configuration
```

### Typical Workflow

1. Set up a systemd timer to run `fuckimlate sync` periodically (e.g., every 30 minutes)
2. Bind `fuckimlate` to a keyboard shortcut in your compositor (e.g., Super+M)
3. When late to a meeting, run `fuckimlate now` and it dials in automatically

### Auto-sync

`pick` and `now` automatically trigger a sync if local data is older than 60 minutes (configurable via `sync.stale_threshold_minutes`).

## Default Handlers

| Conference Type | Command | Notes |
|----------------|---------|-------|
| Zoom | `xdg-open` | Native client picks up the URL |
| Teams | `google-chrome-stable {url}` | Works better in Chromium |
| Google Meet | `google-chrome-stable {url}` | Works better in Chromium |
| Slack | `xdg-open` | Slack client handles its URLs |
| WebEx | `xdg-open` | |
| Other | `xdg-open` | Fallback |

Override any handler in your config file under `[handlers.<type>]`.

## Icons

When using fuzzel, meeting entries display conference-type icons from your installed icon theme. The following icon names are used (with fallbacks):

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

- `fuzzel` (Wayland dmenu) — preferred picker; if missing, falls back to `rofi`, then plain terminal input
- `notify-send` (desktop notifications)
- `xdg-utils` (xdg-open)
- D-Bus + a secret service provider (GNOME Keyring, KWallet, etc.)
- A Chromium-based browser (optional, for Teams/Meet defaults)

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

Or use the standalone derivation in `nix/package.nix` with `fetchFromGitHub`.

## License

MIT
