// Types in this module are foundational and will be consumed by other modules
// (storage, extract, calendar, ui) as they are implemented.
#![allow(dead_code)]

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
    UnknownConferenceType {
        /// The unrecognized string value.
        value: String,
    },
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
