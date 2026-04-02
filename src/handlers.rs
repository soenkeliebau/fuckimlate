// Types in this module are not yet wired into the main entry point.
#![allow(dead_code)]

use std::process::{Command, Stdio};

use snafu::{ResultExt, Snafu};

use crate::config::{Config, HandlerConfig};
use crate::model::Meeting;

/// Error type for the handlers module.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
    /// The meeting has no conference URL to open.
    #[snafu(display("Meeting '{title}' (id={meeting_id}) has no conference URL"))]
    NoConferenceUrl {
        /// The calendar event ID.
        meeting_id: String,
        /// The meeting title.
        title: String,
    },

    /// Failed to spawn the handler process.
    #[snafu(display("Failed to spawn handler command '{command}'"))]
    SpawnHandler {
        /// The command that failed to launch.
        command: String,
        /// The underlying I/O error.
        source: std::io::Error,
    },

    /// Failed to spawn the `notify-send` process.
    #[snafu(display("Failed to spawn notify-send"))]
    SendNotification {
        /// The underlying I/O error.
        source: std::io::Error,
    },
}

/// Result type alias for the handlers module.
pub type Result<T> = std::result::Result<T, Error>;

/// Substitutes the `{url}` placeholder in each handler argument with the actual meeting URL.
///
/// Arguments that do not contain `{url}` are passed through unchanged.
pub fn build_args(handler: &HandlerConfig, url: &str) -> Vec<String> {
    handler
        .args
        .iter()
        .map(|arg| arg.replace("{url}", url))
        .collect()
}

/// Launches the appropriate conference handler for the given meeting.
///
/// Resolves the handler via [`Config::handler_for_type`], substitutes the meeting URL into
/// the handler's argument template, and spawns the process in a fire-and-forget manner
/// (stdin, stdout, and stderr are all redirected to null).
///
/// # Errors
///
/// Returns [`Error::NoConferenceUrl`] if the meeting has no `conference_url`.
/// Returns [`Error::SpawnHandler`] if the handler process cannot be spawned.
pub fn launch_meeting(meeting: &Meeting, config: &Config) -> Result<()> {
    let url = meeting
        .conference_url
        .as_deref()
        .ok_or_else(|| Error::NoConferenceUrl {
            meeting_id: meeting.id.clone(),
            title: meeting.title.clone(),
        })?;

    let conference_type = meeting
        .conference_type
        .unwrap_or(crate::model::ConferenceType::Unknown);

    let handler = config.handler_for_type(conference_type);
    let args = build_args(handler, url);

    Command::new(&handler.command)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context(SpawnHandlerSnafu {
            command: &handler.command,
        })?;

    Ok(())
}

/// Sends a desktop notification using `notify-send`.
///
/// The notification is spawned as a fire-and-forget process with the application name
/// `"fuckimlate"` as the summary.
///
/// # Errors
///
/// Returns [`Error::SendNotification`] if `notify-send` cannot be spawned.
pub fn notify_error(message: &str) -> Result<()> {
    Command::new("notify-send")
        .arg("fuckimlate")
        .arg(message)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context(SendNotificationSnafu)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HandlerConfig;

    #[test]
    fn substitute_url_in_args() {
        let handler = HandlerConfig {
            command: "google-chrome".to_owned(),
            args: vec!["--app={url}".to_owned()],
        };
        let args = build_args(&handler, "https://meet.google.com/abc");
        assert_eq!(args, vec!["--app=https://meet.google.com/abc"]);
    }

    #[test]
    fn substitute_url_multiple_args() {
        let handler = HandlerConfig {
            command: "zoom".to_owned(),
            args: vec!["--url".to_owned(), "{url}".to_owned()],
        };
        let args = build_args(&handler, "https://zoom.us/j/123");
        assert_eq!(args, vec!["--url", "https://zoom.us/j/123"]);
    }

    #[test]
    fn substitute_url_no_placeholder() {
        let handler = HandlerConfig {
            command: "xdg-open".to_owned(),
            args: vec!["--flag".to_owned()],
        };
        let args = build_args(&handler, "https://example.com");
        assert_eq!(args, vec!["--flag"]);
    }

    #[test]
    fn launch_meeting_no_conference_url_returns_error() {
        use crate::config::Config;
        use crate::model::Meeting;
        use chrono::Local;

        let config = Config::default();
        let meeting = Meeting {
            id: "evt-1".to_owned(),
            title: "Standup".to_owned(),
            start_time: Local::now(),
            end_time: Local::now(),
            conference_url: None,
            conference_type: Some(crate::model::ConferenceType::Zoom),
            raw_location: None,
            raw_description: None,
        };

        let result = launch_meeting(&meeting, &config);
        assert!(result.is_err());
        let err_msg = format!("{}", result.expect_err("should be an error"));
        assert!(
            err_msg.contains("Standup"),
            "Error message should contain meeting title, got: {err_msg}"
        );
    }
}
