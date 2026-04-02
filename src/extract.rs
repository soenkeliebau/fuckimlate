// Conference URL extraction and type detection.
//
// This module provides functions to detect conference system types from URLs
// and extract conference dial-in information from calendar event fields.

use regex::Regex;
use std::sync::LazyLock;
use url::Url;

use crate::model::ConferenceType;

/// Regex pattern that matches HTTP and HTTPS URLs in free-form text.
static URL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    // SAFETY: this regex is a compile-time constant and is known to be valid.
    #[allow(clippy::expect_used)]
    Regex::new(r#"https?://[^\s<>"']+"#).expect("URL regex is valid")
});

/// Detects the conference system type from a URL string.
///
/// Parses the input as a URL and matches the host against known conference
/// provider domains. Returns `None` if the input is not a valid URL. Returns
/// `Some(ConferenceType::Unknown)` for valid URLs with unrecognized domains.
///
/// # Examples
///
/// ```
/// # use fuckimlate::extract::detect_conference_type;
/// # use fuckimlate::model::ConferenceType;
/// assert_eq!(
///     detect_conference_type("https://us02web.zoom.us/j/123"),
///     Some(ConferenceType::Zoom),
/// );
/// ```
pub fn detect_conference_type(input: &str) -> Option<ConferenceType> {
    let parsed = Url::parse(input).ok()?;
    let host = parsed.host_str()?;

    let conference_type = if host == "zoom.us" || host.ends_with(".zoom.us") {
        ConferenceType::Zoom
    } else if host == "teams.microsoft.com" {
        ConferenceType::Teams
    } else if host == "meet.google.com" {
        ConferenceType::GoogleMeet
    } else if host == "slack.com" || host.ends_with(".slack.com") {
        ConferenceType::Slack
    } else if host == "webex.com" || host.ends_with(".webex.com") {
        ConferenceType::WebEx
    } else {
        ConferenceType::Unknown
    };

    Some(conference_type)
}

/// Extracts a conference URL and its type from free-form text.
///
/// Scans the input text for all URLs using a regex pattern, then classifies
/// each one. Prefers the first URL with a known conference type over unknown
/// URLs. If no known-type URL is found, returns the first URL classified as
/// `Unknown`. Returns `None` if the text contains no URLs at all.
pub fn extract_conference_from_text(text: &str) -> Option<(String, ConferenceType)> {
    let mut first_unknown: Option<(String, ConferenceType)> = None;

    for url_match in URL_REGEX.find_iter(text) {
        let url_str = url_match.as_str();
        if let Some(ct) = detect_conference_type(url_str) {
            if ct != ConferenceType::Unknown {
                return Some((url_str.to_owned(), ct));
            }
            if first_unknown.is_none() {
                first_unknown = Some((url_str.to_owned(), ct));
            }
        }
    }

    first_unknown
}

/// Extracts conference dial-in information using a priority chain.
///
/// Checks the following sources in order and returns the first match:
///
/// 1. **Structured conference data** — looks for a `conferenceData` object with
///    an `entryPoints` array containing an entry whose `entryPointType` is
///    `"video"`. Uses the `uri` field of that entry.
/// 2. **Location field** — scans the location text for URLs via
///    [`extract_conference_from_text`].
/// 3. **Description field** — scans the description text for URLs via
///    [`extract_conference_from_text`].
///
/// Returns `None` if no conference information is found in any source.
pub fn extract_conference_info(
    conference_data: Option<&serde_json::Value>,
    location: Option<&str>,
    description: Option<&str>,
) -> Option<(String, ConferenceType)> {
    // Priority 1: structured conference data with video entry point.
    if let Some(data) = conference_data
        && let Some(entry_points) = data.get("entryPoints").and_then(|v| v.as_array())
    {
        for entry in entry_points {
            let is_video = entry
                .get("entryPointType")
                .and_then(|v| v.as_str())
                .is_some_and(|t| t == "video");

            if is_video
                && let Some(uri) = entry.get("uri").and_then(|v| v.as_str())
                && let Some(ct) = detect_conference_type(uri)
            {
                return Some((uri.to_owned(), ct));
            }
        }
    }

    // Priority 2: location field.
    if let Some(loc) = location
        && let Some(result) = extract_conference_from_text(loc)
    {
        return Some(result);
    }

    // Priority 3: description field.
    if let Some(desc) = description
        && let Some(result) = extract_conference_from_text(desc)
    {
        return Some(result);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_zoom_url() {
        let url = "https://us02web.zoom.us/j/12345678?pwd=abc";
        assert_eq!(detect_conference_type(url), Some(ConferenceType::Zoom));
    }

    #[test]
    fn detect_teams_url() {
        let url = "https://teams.microsoft.com/l/meetup-join/abc123";
        assert_eq!(detect_conference_type(url), Some(ConferenceType::Teams));
    }

    #[test]
    fn detect_google_meet_url() {
        let url = "https://meet.google.com/abc-defg-hij";
        assert_eq!(
            detect_conference_type(url),
            Some(ConferenceType::GoogleMeet)
        );
    }

    #[test]
    fn detect_slack_url() {
        let url = "https://mycompany.slack.com/archives/C123/huddle";
        assert_eq!(detect_conference_type(url), Some(ConferenceType::Slack));
    }

    #[test]
    fn detect_webex_url() {
        let url = "https://mycompany.webex.com/meet/jdoe";
        assert_eq!(detect_conference_type(url), Some(ConferenceType::WebEx));
    }

    #[test]
    fn detect_unknown_url() {
        let url = "https://example.com/meeting/123";
        assert_eq!(detect_conference_type(url), Some(ConferenceType::Unknown));
    }

    #[test]
    fn detect_invalid_url_returns_none() {
        let url = "not a url at all";
        assert_eq!(detect_conference_type(url), None);
    }

    #[test]
    fn extract_url_from_text_finds_zoom() {
        let text = "Join us at https://us02web.zoom.us/j/123 for the meeting. See you!";
        let (url, ct) = extract_conference_from_text(text).expect("should find URL");
        assert!(url.contains("zoom.us"));
        assert_eq!(ct, ConferenceType::Zoom);
    }

    #[test]
    fn extract_url_prefers_known_type_over_unknown() {
        let text = "Link: https://example.com/foo also https://meet.google.com/abc-def-ghi";
        let (url, ct) = extract_conference_from_text(text).expect("should find URL");
        assert_eq!(ct, ConferenceType::GoogleMeet);
        assert!(url.contains("meet.google.com"));
    }

    #[test]
    fn extract_url_from_text_no_urls() {
        let text = "No URLs here, just plain text about tomorrow's meeting.";
        assert!(extract_conference_from_text(text).is_none());
    }

    #[test]
    fn extract_conference_info_from_conference_data() {
        let data = serde_json::json!({
            "entryPoints": [
                {
                    "entryPointType": "phone",
                    "uri": "tel:+1-555-123-4567"
                },
                {
                    "entryPointType": "video",
                    "uri": "https://meet.google.com/abc-defg-hij"
                }
            ]
        });
        let (url, ct) =
            extract_conference_info(Some(&data), None, None).expect("should find video entry");
        assert!(url.contains("meet.google.com"));
        assert_eq!(ct, ConferenceType::GoogleMeet);
    }

    #[test]
    fn extract_conference_info_falls_back_to_location() {
        let (url, ct) =
            extract_conference_info(None, Some("https://us02web.zoom.us/j/123?pwd=abc"), None)
                .expect("should find URL in location");
        assert!(url.contains("zoom.us"));
        assert_eq!(ct, ConferenceType::Zoom);
    }

    #[test]
    fn extract_conference_info_falls_back_to_description() {
        let (url, ct) = extract_conference_info(
            None,
            None,
            Some("Meeting link: https://teams.microsoft.com/l/meetup-join/abc"),
        )
        .expect("should find URL in description");
        assert!(url.contains("teams.microsoft.com"));
        assert_eq!(ct, ConferenceType::Teams);
    }

    #[test]
    fn extract_conference_info_conference_data_wins_over_location() {
        let data = serde_json::json!({
            "entryPoints": [
                {
                    "entryPointType": "video",
                    "uri": "https://meet.google.com/abc-defg-hij"
                }
            ]
        });
        let (url, ct) =
            extract_conference_info(Some(&data), Some("https://us02web.zoom.us/j/123"), None)
                .expect("should find conference data entry");
        assert_eq!(ct, ConferenceType::GoogleMeet);
        assert!(url.contains("meet.google.com"));
    }

    #[test]
    fn extract_conference_info_none_when_no_info() {
        assert!(extract_conference_info(None, None, None).is_none());
    }
}
