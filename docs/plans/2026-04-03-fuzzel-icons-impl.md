# Fuzzel Conference Icons Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Show conference-type icons (Zoom, Teams, Meet, etc.) next to meeting entries in the fuzzel picker.

**Architecture:** Add an `icon_name()` method to `ConferenceType` returning XDG icon theme names with fallbacks. Introduce a fuzzel-specific formatting function in `ui.rs` that appends the `\0icon\x1f<name>` suffix. Update `parse_selection` to strip icon metadata before matching. Only the fuzzel code path uses icon formatting; rofi and terminal are unchanged.

**Tech Stack:** Rust, fuzzel dmenu protocol (Rofi extended dmenu icon format: `\0icon\x1f<name>`)

---

### Task 1: Add `icon_name()` to `ConferenceType`

**Files:**
- Modify: `src/model.rs:22-50` (the `ConferenceType` enum and its impls)

**Step 1: Write the failing test**

In `src/model.rs`, inside the existing `#[cfg(test)] mod tests` block (after the `conference_type_from_str_invalid` test at line 118), add:

```rust
#[test]
fn conference_type_icon_names() {
    assert_eq!(ConferenceType::Zoom.icon_name(), "Zoom,video-call");
    assert_eq!(ConferenceType::Teams.icon_name(), "teams-for-linux,video-call");
    assert_eq!(ConferenceType::GoogleMeet.icon_name(), "google-meet,video-call");
    assert_eq!(ConferenceType::Slack.icon_name(), "slack,chat");
    assert_eq!(ConferenceType::WebEx.icon_name(), "webex,video-call");
    assert_eq!(ConferenceType::Unknown.icon_name(), "video-call");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --all-features conference_type_icon_names -- --nocapture`
Expected: FAIL — `icon_name` method not found on `ConferenceType`.

**Step 3: Implement `icon_name()` on `ConferenceType`**

In `src/model.rs`, add a new `impl` block after the `FromStr` impl (after line 69):

```rust
impl ConferenceType {
    /// Returns the XDG icon theme name(s) for this conference type.
    ///
    /// Names are comma-separated for fuzzel fallback support: the first name
    /// is the preferred icon, subsequent names are tried if the preferred icon
    /// is not found in the current icon theme.
    pub fn icon_name(self) -> &'static str {
        match self {
            Self::Zoom => "Zoom,video-call",
            Self::Teams => "teams-for-linux,video-call",
            Self::GoogleMeet => "google-meet,video-call",
            Self::Slack => "slack,chat",
            Self::WebEx => "webex,video-call",
            Self::Unknown => "video-call",
        }
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --all-features conference_type_icon_names -- --nocapture`
Expected: PASS

**Step 5: Run full checks**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features`
Expected: All pass, zero warnings.

**Step 6: Commit**

```bash
git add src/model.rs
git commit -m "feat: add icon_name() method to ConferenceType for XDG icon lookup"
```

---

### Task 2: Add fuzzel-specific icon formatting in `ui.rs`

**Files:**
- Modify: `src/ui.rs:49-68` (formatting functions area)

**Step 1: Write the failing tests**

In `src/ui.rs`, inside the existing `#[cfg(test)] mod tests` block (after the `parse_selection_no_match` test at line 264), add:

```rust
#[test]
fn format_meeting_with_icon_includes_conference_icon() {
    let m = make_meeting("Standup", 9, true);
    let line = format_meeting_with_icon(&m);
    assert_eq!(
        line,
        "[09:00] - Standup\0icon\x1fgoogle-meet,video-call"
    );
}

#[test]
fn format_meeting_with_icon_no_conference_type() {
    let m = make_meeting("Lunch", 12, false);
    let line = format_meeting_with_icon(&m);
    assert_eq!(line, "[12:00] ? Lunch\0icon\x1fappointment-soon");
}

#[test]
fn format_meeting_list_with_icons_joins_with_newline() {
    let meetings = vec![
        make_meeting("Standup", 9, true),
        make_meeting("Lunch", 12, false),
    ];
    let lines = format_meeting_list_with_icons(&meetings);
    assert_eq!(
        lines,
        "[09:00] - Standup\0icon\x1fgoogle-meet,video-call\n[12:00] ? Lunch\0icon\x1fappointment-soon"
    );
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --all-features format_meeting_with_icon -- --nocapture`
Expected: FAIL — `format_meeting_with_icon` not found.

**Step 3: Implement the icon formatting functions**

In `src/ui.rs`, after the `format_meeting_list` function (after line 68), add:

```rust
/// Formats a single meeting with a fuzzel icon suffix.
///
/// Appends the `\0icon\x1f<icon-name>` suffix that fuzzel uses to display
/// icons next to entries in dmenu mode. The icon is based on the meeting's
/// conference type, falling back to `appointment-soon` for meetings without
/// a detected conference system.
///
/// This format is fuzzel-specific and should not be used with rofi or terminal pickers.
pub fn format_meeting_with_icon(meeting: &Meeting) -> String {
    let base = format_meeting(meeting);
    let icon = meeting
        .conference_type
        .map_or("appointment-soon", |ct| ct.icon_name());
    format!("{base}\0icon\x1f{icon}")
}

/// Formats a list of meetings with fuzzel icon suffixes, joined by newlines.
///
/// Each meeting is formatted using [`format_meeting_with_icon`].
pub fn format_meeting_list_with_icons(meetings: &[Meeting]) -> String {
    meetings
        .iter()
        .map(format_meeting_with_icon)
        .collect::<Vec<_>>()
        .join("\n")
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --all-features format_meeting_with_icon -- --nocapture`
Expected: PASS (all 3 new tests)

**Step 5: Run full checks**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features`
Expected: All pass, zero warnings.

**Step 6: Commit**

```bash
git add src/ui.rs
git commit -m "feat: add fuzzel icon formatting for meeting entries"
```

---

### Task 3: Update `parse_selection` to strip icon metadata

**Files:**
- Modify: `src/ui.rs:74-76` (`parse_selection` function)

**Step 1: Write the failing test**

In `src/ui.rs` tests, add after the new icon tests:

```rust
#[test]
fn parse_selection_strips_icon_suffix() {
    let meetings = vec![
        make_meeting("Standup", 9, true),
        make_meeting("Lunch", 12, false),
    ];
    // fuzzel returns the full line including the icon metadata
    let selected = parse_selection(
        "[09:00] - Standup\0icon\x1fgoogle-meet,video-call",
        &meetings,
    );
    assert!(selected.is_some());
    assert_eq!(selected.expect("should find meeting").title, "Standup");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --all-features parse_selection_strips_icon -- --nocapture`
Expected: FAIL — `parse_selection` returns `None` because the icon suffix doesn't match.

**Step 3: Update `parse_selection` to strip `\0` prefix**

Replace the `parse_selection` function body in `src/ui.rs` (lines 74-76):

```rust
/// Matches a picker selection line back to a meeting in the provided list.
///
/// Strips any fuzzel icon metadata (everything from the first `\0` onward)
/// before matching against formatted meeting lines. This allows the function
/// to work with both plain and icon-annotated picker output.
pub fn parse_selection<'a>(line: &str, meetings: &'a [Meeting]) -> Option<&'a Meeting> {
    let clean = line.split('\0').next().unwrap_or(line);
    meetings.iter().find(|m| format_meeting(m) == clean)
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --all-features parse_selection -- --nocapture`
Expected: PASS — both the old `parse_selection_finds_meeting`, `parse_selection_no_match`, and the new `parse_selection_strips_icon_suffix` pass.

**Step 5: Run full checks**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features`
Expected: All pass, zero warnings.

**Step 6: Commit**

```bash
git add src/ui.rs
git commit -m "fix: strip fuzzel icon metadata from selection before matching"
```

---

### Task 4: Wire icon formatting into the fuzzel picker path

**Files:**
- Modify: `src/ui.rs:88-120` (`pick_with_dmenu` function)
- Modify: `src/ui.rs:170-191` (`pick_meeting` function)

**Step 1: Add an `icons` parameter to `pick_with_dmenu`**

Change the `pick_with_dmenu` signature and body to accept an `icons: bool` parameter that controls whether to use icon-annotated formatting:

```rust
/// Spawns a dmenu-compatible picker (fuzzel or rofi) to let the user choose a meeting.
///
/// When `icons` is `true`, meeting entries include fuzzel icon metadata (the
/// `\0icon\x1f<name>` suffix). This should only be set for fuzzel, as other
/// dmenu-compatible pickers do not support this format.
///
/// # Errors
///
/// Returns [`Error::SpawnPicker`] if the picker process cannot be started.
/// Returns [`Error::PickerIo`] if an I/O error occurs while communicating with the picker.
fn pick_with_dmenu<'a>(
    meetings: &'a [Meeting],
    command: &str,
    args: &[String],
    icons: bool,
) -> Result<Option<&'a Meeting>> {
    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context(SpawnPickerSnafu { command })?;

    let input = if icons {
        format_meeting_list_with_icons(meetings)
    } else {
        format_meeting_list(meetings)
    };

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes()).context(PickerIoSnafu)?;
    }

    let output = child.wait_with_output().context(PickerIoSnafu)?;

    if !output.status.success() {
        return Ok(None);
    }

    let selection = String::from_utf8_lossy(&output.stdout);
    let trimmed = selection.trim();

    if trimmed.is_empty() {
        return Ok(None);
    }

    Ok(parse_selection(trimmed, meetings))
}
```

**Step 2: Update `pick_meeting` to pass `icons: true` for fuzzel, `false` for rofi**

Replace the `pick_meeting` function body:

```rust
pub fn pick_meeting<'a>(meetings: &'a [Meeting], config: &UiConfig) -> Result<Option<&'a Meeting>> {
    // Try fuzzel first (with icons).
    match pick_with_dmenu(meetings, &config.fuzzel_command, &config.fuzzel_args, true) {
        Ok(result) => return Ok(result),
        Err(Error::SpawnPicker { ref source, .. }) if source.kind() == io::ErrorKind::NotFound => {
            debug!("fuzzel not found, trying rofi");
        }
        Err(e) => return Err(e),
    }

    // Try rofi (without icons — rofi uses a different icon protocol).
    match pick_with_dmenu(meetings, &config.rofi_command, &config.rofi_args, false) {
        Ok(result) => return Ok(result),
        Err(Error::SpawnPicker { ref source, .. }) if source.kind() == io::ErrorKind::NotFound => {
            debug!("rofi not found, falling back to terminal");
        }
        Err(e) => return Err(e),
    }

    // Fall back to terminal input.
    pick_with_terminal(meetings)
}
```

**Step 3: Update existing test that calls `pick_with_dmenu` directly**

The test `pick_with_dmenu_not_found_returns_spawn_error` (line 267) calls `pick_with_dmenu` directly. Update to add the `icons` parameter:

Change:
```rust
let result = pick_with_dmenu(&meetings, "nonexistent-picker-binary-xyz", &[]);
```
To:
```rust
let result = pick_with_dmenu(&meetings, "nonexistent-picker-binary-xyz", &[], false);
```

**Step 4: Run full checks**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features`
Expected: All pass, zero warnings.

**Step 5: Commit**

```bash
git add src/ui.rs
git commit -m "feat: wire fuzzel icon formatting into picker pipeline"
```

---

### Task 5: Update documentation

**Files:**
- Modify: `README.md`
- Modify: `config.toml.template`

**Step 1: Add icons section to README**

After the "Default Handlers" table in `README.md`, add a section:

```markdown
## Icons

When using fuzzel, meeting entries display conference-type icons from your installed icon theme. The following icon names are used (with fallbacks):

| Conference Type | Icon name(s) |
|----------------|-------------------------------|
| Zoom | `Zoom`, `video-call` |
| Teams | `teams-for-linux`, `video-call` |
| Google Meet | `google-meet`, `video-call` |
| Slack | `slack`, `chat` |
| WebEx | `webex`, `video-call` |
| Unknown | `video-call` |
| No conference | `appointment-soon` |

Icon themes like [Papirus](https://github.com/PapirusDevelopmentTeam/papirus-icon-theme) provide good coverage. If an icon is not found in your theme, fuzzel silently skips it.
```

**Step 2: Run full checks**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features`
Expected: All pass (no code changes in this task, just docs).

**Step 3: Commit**

```bash
git add README.md config.toml.template
git commit -m "docs: document fuzzel icon support and icon theme names"
```
