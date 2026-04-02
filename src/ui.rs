// Fuzzel UI integration for displaying and selecting meetings.

use std::io::Write;
use std::process::{Command, Stdio};

use snafu::{ResultExt, Snafu};

use crate::config::UiConfig;
use crate::model::Meeting;

/// Error type for the UI module.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
    /// Failed to spawn the fuzzel process.
    #[snafu(display("Failed to spawn fuzzel command '{command}'"))]
    SpawnFuzzel {
        /// The command that was attempted.
        command: String,
        /// The underlying I/O error.
        source: std::io::Error,
    },

    /// I/O error communicating with the fuzzel process.
    #[snafu(display("I/O error communicating with fuzzel"))]
    FuzzelIo {
        /// The underlying I/O error.
        source: std::io::Error,
    },
}

/// Result type alias for the UI module.
pub type Result<T> = std::result::Result<T, Error>;

/// Formats a single meeting as a display line for fuzzel.
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

/// Formats a list of meetings as newline-separated display lines for fuzzel.
///
/// Each meeting is formatted using [`format_meeting`] and joined with newline characters.
pub fn format_meeting_list(meetings: &[Meeting]) -> String {
    meetings
        .iter()
        .map(format_meeting)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Matches a fuzzel selection line back to a meeting in the provided list.
///
/// Compares the selected line against the formatted output of each meeting using
/// [`format_meeting`]. Returns the first matching meeting, or `None` if no match is found.
pub fn parse_selection<'a>(line: &str, meetings: &'a [Meeting]) -> Option<&'a Meeting> {
    meetings.iter().find(|m| format_meeting(m) == line)
}

/// Spawns fuzzel to let the user pick a meeting from the list.
///
/// Pipes the formatted meeting list to fuzzel's stdin and reads the user's
/// selection from stdout. Returns the selected meeting, or `None` if the user
/// dismissed fuzzel without making a selection.
///
/// # Errors
///
/// Returns [`Error::SpawnFuzzel`] if the fuzzel process cannot be started.
/// Returns [`Error::FuzzelIo`] if an I/O error occurs while communicating with fuzzel.
pub fn pick_meeting<'a>(meetings: &'a [Meeting], config: &UiConfig) -> Result<Option<&'a Meeting>> {
    let mut child = Command::new(&config.fuzzel_command)
        .args(&config.fuzzel_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context(SpawnFuzzelSnafu {
            command: config.fuzzel_command.clone(),
        })?;

    let input = format_meeting_list(meetings);

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes()).context(FuzzelIoSnafu)?;
    }

    let output = child.wait_with_output().context(FuzzelIoSnafu)?;

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
}
