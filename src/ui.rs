// Picker UI integration for displaying and selecting meetings.
//
// Uses a fallback chain: fuzzel → rofi → terminal input.

use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

use snafu::{ResultExt, Snafu};
use tracing::debug;

use crate::config::UiConfig;
use crate::model::{ConferenceType, Meeting};

/// Error type for the UI module.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
    /// Failed to spawn a picker process.
    #[snafu(display("Failed to spawn picker command '{command}'"))]
    SpawnPicker {
        /// The command that was attempted.
        command: String,
        /// The underlying I/O error.
        source: io::Error,
    },

    /// I/O error communicating with a picker process.
    #[snafu(display("I/O error communicating with picker"))]
    PickerIo {
        /// The underlying I/O error.
        source: io::Error,
    },
}

/// Result type alias for the UI module.
pub type Result<T> = std::result::Result<T, Error>;

/// Formats a single meeting as a display line for the picker.
///
/// Meetings with a conference URL are shown with a `-` separator, while those
/// without use `?` to indicate no dial-in information is available.
///
/// # Examples
///
/// ```text
/// Meeting with conference URL:  "[09:00] - Standup"
/// Meeting without conference URL: "[12:00] ? Lunch"
/// ```
pub fn format_meeting(meeting: &Meeting) -> String {
    let time = meeting.start_time.format("%H:%M");
    let separator = if meeting.conference_url.is_some() {
        "-"
    } else {
        "?"
    };
    format!("[{time}] {separator} {}", meeting.title)
}

/// Formats a list of meetings as newline-separated display lines for the picker.
///
/// Each meeting is formatted using [`format_meeting`] and joined with newline characters.
pub fn format_meeting_list(meetings: &[Meeting]) -> String {
    meetings
        .iter()
        .map(format_meeting)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Formats a single meeting with a fuzzel icon suffix.
///
/// Appends the `\0icon\x1f<icon-name>` suffix that fuzzel uses to display
/// icons next to entries in dmenu mode. The icon is based on the meeting's
/// conference type, falling back to `appointment-soon` for meetings without
/// a detected conference system.
///
/// This format is fuzzel-specific and should not be used with rofi or terminal pickers.
pub fn format_meeting_with_icon(meeting: &Meeting) -> String {
    let time = meeting.start_time.format("%H:%M");
    let separator = if meeting.conference_url.is_some() {
        "-"
    } else {
        "?"
    };
    let icon = meeting
        .conference_type
        .unwrap_or(ConferenceType::Unknown)
        .icon_name();
    format!("[{time}] {separator} {}\0icon\x1f{icon}", meeting.title)
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

/// Matches a picker selection line back to a meeting in the provided list.
///
/// Strips any fuzzel icon metadata (everything from the first `\0` onward)
/// before matching against formatted meeting lines. This allows the function
/// to work with both plain and icon-annotated picker output.
pub fn parse_selection<'a>(line: &str, meetings: &'a [Meeting]) -> Option<&'a Meeting> {
    let clean = line.split('\0').next().unwrap_or(line);
    meetings.iter().find(|m| format_meeting(m) == clean)
}

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

/// Presents a numbered list of meetings in the terminal and reads the user's choice.
///
/// Prints each meeting with a 1-based index and prompts for input on stdin.
/// Returns the selected meeting, or `None` if the user enters an empty line
/// or an invalid number.
///
/// # Errors
///
/// Returns [`Error::PickerIo`] if an I/O error occurs while reading from stdin
/// or writing to stdout.
fn pick_with_terminal(meetings: &[Meeting]) -> Result<Option<&Meeting>> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    writeln!(out, "Today's meetings:").context(PickerIoSnafu)?;
    for (i, meeting) in meetings.iter().enumerate() {
        writeln!(out, "  {}: {}", i + 1, format_meeting(meeting)).context(PickerIoSnafu)?;
    }
    write!(out, "Select [1-{}]: ", meetings.len()).context(PickerIoSnafu)?;
    out.flush().context(PickerIoSnafu)?;

    let stdin = io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line).context(PickerIoSnafu)?;

    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    match trimmed.parse::<usize>() {
        Ok(n) if n >= 1 && n <= meetings.len() => Ok(Some(&meetings[n - 1])),
        _ => Ok(None),
    }
}

/// Lets the user pick a meeting using the best available picker.
///
/// Tries pickers in order: fuzzel → rofi → terminal input. If a picker binary
/// is not found (`ErrorKind::NotFound`), the next one is tried. Any other error
/// (e.g., the user dismissed the picker, or an I/O failure) is returned immediately.
///
/// # Errors
///
/// Returns [`Error::SpawnPicker`] if a picker was found but failed to start for
/// a reason other than the binary being missing.
/// Returns [`Error::PickerIo`] if an I/O error occurs while communicating with
/// the chosen picker.
pub fn pick_meeting<'a>(meetings: &'a [Meeting], config: &UiConfig) -> Result<Option<&'a Meeting>> {
    let pickers: &[(&str, &[String], bool)] = &[
        (&config.fuzzel_command, &config.fuzzel_args, true),
        (&config.rofi_command, &config.rofi_args, false),
    ];

    for (command, args, icons) in pickers {
        match pick_with_dmenu(meetings, command, args, *icons) {
            Ok(result) => return Ok(result),
            Err(Error::SpawnPicker { ref source, .. })
                if source.kind() == io::ErrorKind::NotFound =>
            {
                debug!("{command} not found, trying next picker");
            }
            Err(e) => return Err(e),
        }
    }

    // Fall back to terminal input.
    pick_with_terminal(meetings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ConferenceType;
    use chrono::TimeZone;

    fn make_meeting(title: &str, hour: u32, has_url: bool) -> Meeting {
        let start = chrono::Local
            .with_ymd_and_hms(2026, 4, 2, hour, 0, 0)
            .unwrap();
        let end = chrono::Local
            .with_ymd_and_hms(2026, 4, 2, hour + 1, 0, 0)
            .unwrap();
        Meeting {
            id: "1".to_owned(),
            title: title.to_owned(),
            start_time: start,
            end_time: end,
            conference_url: if has_url {
                Some("https://meet.google.com/abc".to_owned())
            } else {
                None
            },
            conference_type: if has_url {
                Some(ConferenceType::GoogleMeet)
            } else {
                None
            },
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
    fn format_meeting_list_joins_with_newline() {
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

    #[test]
    fn parse_selection_strips_icon_suffix() {
        let meetings = vec![
            make_meeting("Standup", 9, true),
            make_meeting("Lunch", 12, false),
        ];
        // fuzzel returns the full line including the icon metadata
        let selected = parse_selection("[09:00] - Standup\0icon\x1fgoogle-meet", &meetings);
        assert!(selected.is_some());
        assert_eq!(selected.expect("should find meeting").title, "Standup");
    }

    #[test]
    fn pick_with_dmenu_not_found_returns_spawn_error() {
        let meetings = vec![make_meeting("Standup", 9, true)];
        let result = pick_with_dmenu(&meetings, "nonexistent-picker-binary-xyz", &[], false);
        let err = result.expect_err("should fail for missing binary");
        match err {
            Error::SpawnPicker { ref source, .. } => {
                assert_eq!(source.kind(), io::ErrorKind::NotFound);
            }
            other => panic!("expected SpawnPicker, got {other:?}"),
        }
    }

    #[test]
    fn format_meeting_with_icon_includes_conference_icon() {
        let m = make_meeting("Standup", 9, true);
        let line = format_meeting_with_icon(&m);
        assert_eq!(line, "[09:00] - Standup\0icon\x1fgoogle-meet");
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
            "[09:00] - Standup\0icon\x1fgoogle-meet\n[12:00] ? Lunch\0icon\x1fappointment-soon"
        );
    }

    #[test]
    fn pick_meeting_falls_through_to_terminal_stub() {
        // With both pickers set to nonexistent binaries, pick_meeting should
        // attempt the terminal fallback. We can't easily test interactive stdin
        // here, but we verify the dmenu fallback logic doesn't panic.
        let config = UiConfig {
            fuzzel_command: "nonexistent-fuzzel-xyz".to_owned(),
            fuzzel_args: vec![],
            rofi_command: "nonexistent-rofi-xyz".to_owned(),
            rofi_args: vec![],
        };
        let meetings = vec![make_meeting("Standup", 9, true)];
        // This will try terminal input. Since stdin is not a TTY in tests,
        // it will read EOF and return None.
        let result = pick_meeting(&meetings, &config);
        assert!(result.is_ok());
        assert!(result.expect("should succeed").is_none());
    }
}
