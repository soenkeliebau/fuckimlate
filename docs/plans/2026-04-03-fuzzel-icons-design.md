# Fuzzel Icons for Conference Types

## Problem

The fuzzel picker menu shows meeting entries as plain text. Adding conference-type icons makes it easier to visually distinguish Zoom, Teams, Meet, etc. at a glance.

## Approach

Fuzzel supports icons in dmenu mode via the Rofi extended dmenu protocol. Each line can have `\0icon\x1f<icon-name>` appended, and fuzzel resolves the icon via XDG icon theme lookup. Comma-separated names provide fallback: `Zoom,video-call` tries `Zoom` first, then `video-call`.

Icons are sourced from the user's installed icon theme (e.g. Papirus, Adwaita). No bundled assets.

## Icon Mapping

| ConferenceType | Icon name(s)                  |
|----------------|-------------------------------|
| Zoom           | `Zoom,video-call`             |
| Teams          | `teams-for-linux,video-call`  |
| GoogleMeet     | `google-meet,video-call`      |
| Slack          | `slack,chat`                  |
| WebEx          | `webex,video-call`            |
| Unknown        | `video-call`                  |
| No conference  | `appointment-soon`            |

## Changes

### 1. `ConferenceType::icon_name()` method

Returns `&'static str` with the comma-separated fallback icon name string.

### 2. Fuzzel-specific icon formatting

A new function `format_meeting_with_icon(meeting) -> String` appends the `\0icon\x1f{icon}` suffix to the formatted meeting line. This is only used when piping to fuzzel — rofi and terminal paths continue to use the plain `format_meeting()`.

### 3. `parse_selection()` update

Strip the `\0icon\x1f...` suffix before matching, since fuzzel returns the full line including the icon metadata in its output. Alternatively, match only up to `\0`.

### 4. Icon for meetings without a conference type

Use `appointment-soon` (a standard icon in most themes) for meetings that have no detected conference system.

## Non-Changes

- No bundled icons — relies entirely on installed icon themes.
- No config toggle — the icon suffix is only emitted for fuzzel, which is the only picker that supports it. Rofi and terminal paths are unaffected.
- No rofi icon support — rofi uses a different icon protocol; out of scope for now.
