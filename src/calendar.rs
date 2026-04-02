// Google Calendar API client with OAuth2 authentication and event parsing.
//
// This module provides functions to fetch events from Google Calendar,
// parse the API responses into `Meeting` structs, and handle the OAuth2
// authorization flow for API access.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write as IoWrite};
use std::net::TcpListener;

use chrono::{DateTime, Local, NaiveDate, TimeZone};
use snafu::{ResultExt, Snafu};
use tracing::{debug, info, warn};

use crate::config::CalendarConfig;
use crate::extract::extract_conference_info;
use crate::model::Meeting;

/// Error type for the calendar module.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
    /// OAuth2 client credentials are not configured.
    #[snafu(display(
        "Missing OAuth2 credentials: client_id and client_secret must be set in config"
    ))]
    MissingCredentials,

    /// An HTTP request to the Google API failed.
    #[snafu(display("HTTP request to {url} failed"))]
    HttpRequest {
        /// The URL that was requested.
        url: String,
        /// The underlying reqwest error.
        source: reqwest::Error,
    },

    /// Google API returned a non-success HTTP status.
    #[snafu(display("Google API returned status {status}: {body}"))]
    ApiRejection {
        /// The HTTP status code.
        status: u16,
        /// The response body text.
        body: String,
    },

    /// Failed to parse a field from the API response JSON.
    #[snafu(display("Failed to parse API response: {detail}"))]
    ParseResponse {
        /// Description of what was expected or missing.
        detail: String,
    },

    /// Failed to parse a date-time value from an event.
    #[snafu(display("Failed to parse date-time value: {value}"))]
    ParseDateTime {
        /// The string value that could not be parsed.
        value: String,
    },

    /// Failed to read or write the refresh token in the system keyring.
    #[snafu(display("Keyring operation failed: {detail}"))]
    Keyring {
        /// Description of the keyring error.
        detail: String,
    },

    /// Failed to start the local TCP listener for the OAuth2 callback.
    #[snafu(display("Failed to start local OAuth2 callback server"))]
    LocalServer {
        /// The underlying I/O error.
        source: std::io::Error,
    },

    /// Failed to open the browser for OAuth2 authorization.
    #[snafu(display("Failed to open browser via xdg-open"))]
    OpenBrowser {
        /// The underlying I/O error.
        source: std::io::Error,
    },

    /// The OAuth2 redirect did not contain an authorization code.
    #[snafu(display("OAuth2 redirect did not contain an authorization code"))]
    AuthCodeNotFound,

    /// Failed to read the response body from Google API.
    #[snafu(display("Failed to read response body from {url}"))]
    ReadResponseBody {
        /// The URL that was requested.
        url: String,
        /// The underlying reqwest error.
        source: reqwest::Error,
    },
}

/// Result type alias for the calendar module.
pub type Result<T> = std::result::Result<T, Error>;

/// Parses a single Google Calendar event JSON object into a [`Meeting`].
///
/// Returns `None` for all-day events (events that have `start.date` instead of
/// `start.dateTime`). Uses [`extract_conference_info`] to detect conference
/// URLs and types from the event's conference data, location, and description
/// fields.
///
/// # Examples
///
/// ```ignore
/// let json = serde_json::json!({
///     "id": "abc123",
///     "summary": "Team Standup",
///     "start": { "dateTime": "2026-04-02T09:00:00+02:00" },
///     "end": { "dateTime": "2026-04-02T09:30:00+02:00" }
/// });
/// let meeting = parse_event(&json).expect("should parse");
/// assert_eq!(meeting.title, "Team Standup");
/// ```
pub fn parse_event(json: &serde_json::Value) -> Option<Meeting> {
    let start_obj = json.get("start")?;

    // All-day events have "date" instead of "dateTime" — skip them.
    if start_obj.get("date").is_some() && start_obj.get("dateTime").is_none() {
        return None;
    }

    let start_str = start_obj.get("dateTime")?.as_str()?;
    let end_str = json.get("end")?.get("dateTime")?.as_str()?;

    let start_time = DateTime::parse_from_rfc3339(start_str)
        .ok()?
        .with_timezone(&Local);
    let end_time = DateTime::parse_from_rfc3339(end_str)
        .ok()?
        .with_timezone(&Local);

    let id = json.get("id")?.as_str()?.to_owned();
    let title = json
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("(No title)")
        .to_owned();

    let location = json.get("location").and_then(|v| v.as_str());
    let description = json.get("description").and_then(|v| v.as_str());
    let conference_data = json.get("conferenceData");

    let (conference_url, conference_type) =
        match extract_conference_info(conference_data, location, description) {
            Some((url, ct)) => (Some(url), Some(ct)),
            None => (None, None),
        };

    Some(Meeting {
        id,
        title,
        start_time,
        end_time,
        conference_url,
        conference_type,
        raw_location: location.map(|s| s.to_owned()),
        raw_description: description.map(|s| s.to_owned()),
    })
}

/// Parses the `items` array from a Google Calendar `events.list` API response.
///
/// Iterates over each event in the `items` array, calls [`parse_event`] on
/// each, and filters out `None` results (all-day events or unparseable events).
///
/// # Errors
///
/// Returns [`Error::ParseResponse`] if the JSON does not contain an `items`
/// array.
pub fn parse_events_list(json: &serde_json::Value) -> Result<Vec<Meeting>> {
    let items = json
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or_else(|| Error::ParseResponse {
            detail: "response missing 'items' array".to_owned(),
        })?;

    let meetings = items.iter().filter_map(parse_event).collect();
    Ok(meetings)
}

/// Google Calendar API client that fetches events using an OAuth2 access token.
pub struct CalendarClient {
    /// The HTTP client used for API requests.
    client: reqwest::blocking::Client,
    /// The OAuth2 access token for authentication.
    access_token: String,
}

impl CalendarClient {
    /// Creates a new [`CalendarClient`] with the given access token.
    pub fn new(access_token: String) -> Self {
        Self {
            client: reqwest::blocking::Client::new(),
            access_token,
        }
    }

    /// Fetches today's events from a specific Google Calendar.
    ///
    /// Calls the Google Calendar `events.list` endpoint with time boundaries
    /// set to today (local timezone), requesting single events sorted by start
    /// time. Conference data is requested via `conferenceDataVersion=1`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::HttpRequest`] if the HTTP request fails.
    /// Returns [`Error::ApiRejection`] if the API returns a non-2xx status.
    /// Returns [`Error::ReadResponseBody`] if the response body cannot be read.
    /// Returns [`Error::ParseResponse`] if the response JSON is invalid.
    pub fn fetch_today(&self, calendar_id: &str) -> Result<Vec<Meeting>> {
        let today = Local::now().date_naive();
        let tomorrow = today.succ_opt().unwrap_or(today);

        let time_min = naive_date_to_rfc3339_local(today);
        let time_max = naive_date_to_rfc3339_local(tomorrow);

        let url = format!(
            "https://www.googleapis.com/calendar/v3/calendars/{}/events",
            urlencoding(calendar_id)
        );

        debug!(calendar_id, %time_min, %time_max, "Fetching calendar events");

        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .query(&[
                ("timeMin", time_min.as_str()),
                ("timeMax", time_max.as_str()),
                ("singleEvents", "true"),
                ("orderBy", "startTime"),
                ("conferenceDataVersion", "1"),
            ])
            .send()
            .context(HttpRequestSnafu { url: &url })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(Error::ApiRejection {
                status: status.as_u16(),
                body,
            });
        }

        let body: serde_json::Value = response.json().context(HttpRequestSnafu { url: &url })?;

        parse_events_list(&body)
    }

    /// Fetches today's events from all configured calendars, merged and sorted.
    ///
    /// Calls [`fetch_today`](Self::fetch_today) for each calendar ID, merges
    /// all results, and sorts by start time in ascending order.
    ///
    /// # Errors
    ///
    /// Returns errors from [`fetch_today`](Self::fetch_today) if any calendar
    /// fetch fails.
    pub fn fetch_all_calendars(&self, calendar_ids: &[String]) -> Result<Vec<Meeting>> {
        let mut all_meetings = Vec::new();

        for calendar_id in calendar_ids {
            match self.fetch_today(calendar_id) {
                Ok(meetings) => {
                    debug!(
                        calendar_id,
                        count = meetings.len(),
                        "Fetched calendar events"
                    );
                    all_meetings.extend(meetings);
                }
                Err(e) => {
                    warn!(calendar_id, error = %e, "Failed to fetch calendar, skipping");
                    return Err(e);
                }
            }
        }

        all_meetings.sort_by_key(|m| m.start_time);
        Ok(all_meetings)
    }
}

/// Obtains a valid OAuth2 access token for the Google Calendar API.
///
/// Attempts to load a saved refresh token from the system keyring and exchange
/// it for a new access token. If no refresh token is found or the refresh fails,
/// runs the full OAuth2 authorization code flow.
///
/// # Errors
///
/// Returns [`Error::MissingCredentials`] if `client_id` or `client_secret` are
/// not configured.
/// Returns [`Error::Keyring`] if keyring access fails.
/// Returns [`Error::HttpRequest`] or [`Error::ApiRejection`] if token exchange fails.
/// Returns errors from [`authenticate`] if the full auth flow is needed.
pub fn get_access_token(config: &CalendarConfig) -> Result<String> {
    // Verify credentials are present before attempting any token operations.
    if config.client_id.is_none() || config.client_secret.is_none() {
        return Err(Error::MissingCredentials);
    }

    // Try to use a saved refresh token.
    if let Some(refresh_token) = load_refresh_token()? {
        debug!("Found saved refresh token, attempting to refresh access token");
        match refresh_access_token(config, &refresh_token) {
            Ok(access_token) => return Ok(access_token),
            Err(e) => {
                warn!(error = %e, "Refresh token exchange failed, running full auth flow");
            }
        }
    }

    // Fall back to full authorization flow.
    info!("No valid refresh token available, starting OAuth2 authorization flow");
    authenticate(config)
}

/// Loads the saved refresh token from the system keyring.
///
/// Returns `Ok(None)` if no refresh token has been saved yet.
///
/// # Errors
///
/// Returns [`Error::Keyring`] if the keyring cannot be accessed (other than
/// a "not found" condition).
pub fn load_refresh_token() -> Result<Option<String>> {
    let entry = keyring::Entry::new("fuckimlate", "refresh_token").map_err(|e| Error::Keyring {
        detail: format!("Failed to create keyring entry: {e}"),
    })?;

    match entry.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(Error::Keyring {
            detail: format!("Failed to read refresh token from keyring: {e}"),
        }),
    }
}

/// Saves a refresh token to the system keyring.
///
/// # Errors
///
/// Returns [`Error::Keyring`] if the keyring write fails.
pub fn save_refresh_token(token: &str) -> Result<()> {
    let entry = keyring::Entry::new("fuckimlate", "refresh_token").map_err(|e| Error::Keyring {
        detail: format!("Failed to create keyring entry: {e}"),
    })?;

    entry.set_password(token).map_err(|e| Error::Keyring {
        detail: format!("Failed to save refresh token to keyring: {e}"),
    })
}

/// Runs the full OAuth2 authorization code flow.
///
/// Opens the user's browser to the Google OAuth2 consent screen, starts a
/// local HTTP server to receive the redirect callback, exchanges the
/// authorization code for tokens, and saves the refresh token to the keyring.
///
/// # Errors
///
/// Returns [`Error::MissingCredentials`] if `client_id` or `client_secret` are not set.
/// Returns [`Error::LocalServer`] if the TCP listener cannot be started.
/// Returns [`Error::OpenBrowser`] if `xdg-open` fails to launch.
/// Returns [`Error::AuthCodeNotFound`] if the redirect does not contain a code.
/// Returns [`Error::HttpRequest`] or [`Error::ApiRejection`] if token exchange fails.
/// Returns [`Error::Keyring`] if saving the refresh token fails.
pub fn authenticate(config: &CalendarConfig) -> Result<String> {
    let client_id = config
        .client_id
        .as_deref()
        .ok_or(Error::MissingCredentials)?;
    let client_secret = config
        .client_secret
        .as_deref()
        .ok_or(Error::MissingCredentials)?;

    // Start a local server on a random port to receive the OAuth2 callback.
    let listener = TcpListener::bind("127.0.0.1:0").context(LocalServerSnafu)?;
    let port = listener.local_addr().context(LocalServerSnafu)?.port();

    let redirect_uri = format!("http://127.0.0.1:{port}");

    // Build the authorization URL.
    let auth_url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth\
         ?client_id={client_id}\
         &redirect_uri={redirect_uri}\
         &response_type=code\
         &scope=https://www.googleapis.com/auth/calendar.readonly\
         &access_type=offline\
         &prompt=consent"
    );

    info!("Opening browser for OAuth2 authorization");
    std::process::Command::new("xdg-open")
        .arg(&auth_url)
        .spawn()
        .context(OpenBrowserSnafu)?;

    // Accept the callback connection and extract the authorization code.
    let (mut stream, _addr) = listener.accept().context(LocalServerSnafu)?;
    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .context(LocalServerSnafu)?;

    let code = extract_code_from_request(&request_line).ok_or(Error::AuthCodeNotFound)?;

    // Send a success response to the browser.
    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
                    <html><body><h1>Authentication successful!</h1>\
                    <p>You can close this tab.</p></body></html>";
    let _ = stream.write_all(response.as_bytes());

    // Exchange the authorization code for tokens.
    let client = reqwest::blocking::Client::new();
    let token_url = "https://oauth2.googleapis.com/token";

    let token_response = client
        .post(token_url)
        .form(&[
            ("code", code.as_str()),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("redirect_uri", redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .context(HttpRequestSnafu { url: token_url })?;

    let status = token_response.status();
    if !status.is_success() {
        let body = token_response.text().unwrap_or_default();
        return Err(Error::ApiRejection {
            status: status.as_u16(),
            body,
        });
    }

    let token_json: serde_json::Value = token_response
        .json()
        .context(HttpRequestSnafu { url: token_url })?;

    let access_token = token_json
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::ParseResponse {
            detail: "token response missing 'access_token'".to_owned(),
        })?
        .to_owned();

    // Save the refresh token if provided.
    if let Some(refresh_token) = token_json.get("refresh_token").and_then(|v| v.as_str()) {
        save_refresh_token(refresh_token)?;
        info!("Saved refresh token to keyring");
    }

    Ok(access_token)
}

/// Exchanges a refresh token for a new access token.
///
/// Sends a POST request to the Google OAuth2 token endpoint with
/// `grant_type=refresh_token`.
///
/// # Errors
///
/// Returns [`Error::MissingCredentials`] if `client_id` or `client_secret` are not set.
/// Returns [`Error::HttpRequest`] if the HTTP request fails.
/// Returns [`Error::ApiRejection`] if the token endpoint returns a non-2xx status.
/// Returns [`Error::ParseResponse`] if the response does not contain an access token.
pub fn refresh_access_token(config: &CalendarConfig, refresh_token: &str) -> Result<String> {
    let client_id = config
        .client_id
        .as_deref()
        .ok_or(Error::MissingCredentials)?;
    let client_secret = config
        .client_secret
        .as_deref()
        .ok_or(Error::MissingCredentials)?;

    let client = reqwest::blocking::Client::new();
    let token_url = "https://oauth2.googleapis.com/token";

    let response = client
        .post(token_url)
        .form(&[
            ("refresh_token", refresh_token),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .context(HttpRequestSnafu { url: token_url })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(Error::ApiRejection {
            status: status.as_u16(),
            body,
        });
    }

    let token_json: serde_json::Value = response
        .json()
        .context(HttpRequestSnafu { url: token_url })?;

    let access_token = token_json
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::ParseResponse {
            detail: "token response missing 'access_token'".to_owned(),
        })?
        .to_owned();

    Ok(access_token)
}

/// Extracts the authorization code from an HTTP request line.
///
/// Parses the first line of an HTTP GET request (e.g.,
/// `GET /?code=4/0AX4XfWh... HTTP/1.1`) and returns the value of the `code`
/// query parameter.
fn extract_code_from_request(request_line: &str) -> Option<String> {
    let path = request_line.split_whitespace().nth(1)?;
    let full_url = format!("http://localhost{path}");
    let parsed = url::Url::parse(&full_url).ok()?;
    parsed
        .query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.into_owned())
}

/// Converts a [`NaiveDate`] to an RFC 3339 timestamp string at midnight local time.
fn naive_date_to_rfc3339_local(date: NaiveDate) -> String {
    let naive_dt = date
        .and_hms_opt(0, 0, 0)
        // PANIC SAFETY: midnight is always a valid time.
        .expect("midnight is always valid");
    let local_dt = Local
        .from_local_datetime(&naive_dt)
        .single()
        // PANIC SAFETY: midnight on a known date should be unambiguous.
        .expect("midnight should be unambiguous");
    local_dt.to_rfc3339()
}

/// URL-encodes a string for use in URL path segments.
///
/// This is a simple percent-encoding for the calendar ID in the API URL.
fn urlencoding(input: &str) -> String {
    url::form_urlencoded::byte_serialize(input.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ConferenceType;

    #[test]
    fn parse_event_with_conference_data() {
        let json = serde_json::json!({
            "id": "abc123",
            "summary": "Team Standup",
            "start": { "dateTime": "2026-04-02T09:00:00+02:00" },
            "end": { "dateTime": "2026-04-02T09:30:00+02:00" },
            "conferenceData": {
                "entryPoints": [{
                    "entryPointType": "video",
                    "uri": "https://meet.google.com/abc-def-ghi"
                }]
            }
        });
        let meeting = parse_event(&json).expect("should parse event");
        assert_eq!(meeting.id, "abc123");
        assert_eq!(meeting.title, "Team Standup");
        assert_eq!(
            meeting.conference_url.as_deref(),
            Some("https://meet.google.com/abc-def-ghi")
        );
        assert_eq!(meeting.conference_type, Some(ConferenceType::GoogleMeet));
    }

    #[test]
    fn parse_event_with_location_url() {
        let json = serde_json::json!({
            "id": "def456",
            "summary": "External Meeting",
            "start": { "dateTime": "2026-04-02T14:00:00+02:00" },
            "end": { "dateTime": "2026-04-02T15:00:00+02:00" },
            "location": "https://us02web.zoom.us/j/12345"
        });
        let meeting = parse_event(&json).expect("should parse event");
        assert_eq!(meeting.conference_type, Some(ConferenceType::Zoom));
    }

    #[test]
    fn parse_event_with_url_in_description() {
        let json = serde_json::json!({
            "id": "ghi789",
            "summary": "Vendor Call",
            "start": { "dateTime": "2026-04-02T16:00:00+02:00" },
            "end": { "dateTime": "2026-04-02T17:00:00+02:00" },
            "description": "Join here: https://teams.microsoft.com/l/meetup-join/abc"
        });
        let meeting = parse_event(&json).expect("should parse event");
        assert_eq!(meeting.conference_type, Some(ConferenceType::Teams));
    }

    #[test]
    fn parse_event_no_conference_info() {
        let json = serde_json::json!({
            "id": "jkl012",
            "summary": "1:1 Walk",
            "start": { "dateTime": "2026-04-02T11:00:00+02:00" },
            "end": { "dateTime": "2026-04-02T11:30:00+02:00" }
        });
        let meeting = parse_event(&json).expect("should parse event");
        assert!(meeting.conference_url.is_none());
        assert!(meeting.conference_type.is_none());
    }

    #[test]
    fn parse_event_all_day_returns_none() {
        let json = serde_json::json!({
            "id": "allday1",
            "summary": "Company Holiday",
            "start": { "date": "2026-04-02" },
            "end": { "date": "2026-04-03" }
        });
        let result = parse_event(&json);
        assert!(result.is_none(), "all-day events should be filtered out");
    }

    #[test]
    fn parse_events_list_response() {
        let json = serde_json::json!({
            "items": [
                {
                    "id": "1",
                    "summary": "Morning",
                    "start": { "dateTime": "2026-04-02T09:00:00+02:00" },
                    "end": { "dateTime": "2026-04-02T10:00:00+02:00" }
                },
                {
                    "id": "allday",
                    "summary": "Holiday",
                    "start": { "date": "2026-04-02" },
                    "end": { "date": "2026-04-03" }
                },
                {
                    "id": "2",
                    "summary": "Afternoon",
                    "start": { "dateTime": "2026-04-02T14:00:00+02:00" },
                    "end": { "dateTime": "2026-04-02T15:00:00+02:00" }
                }
            ]
        });
        let meetings = parse_events_list(&json).expect("should parse list");
        assert_eq!(meetings.len(), 2, "all-day event should be filtered");
        assert_eq!(meetings[0].title, "Morning");
        assert_eq!(meetings[1].title, "Afternoon");
    }

    #[test]
    fn parse_events_list_missing_items_returns_error() {
        let json = serde_json::json!({});
        let result = parse_events_list(&json);
        assert!(result.is_err());
    }

    #[test]
    fn parse_event_with_no_summary_uses_default() {
        let json = serde_json::json!({
            "id": "nosummary",
            "start": { "dateTime": "2026-04-02T10:00:00+02:00" },
            "end": { "dateTime": "2026-04-02T11:00:00+02:00" }
        });
        let meeting = parse_event(&json).expect("should parse event");
        assert_eq!(meeting.title, "(No title)");
    }

    #[test]
    fn extract_code_from_request_line() {
        let line = "GET /?code=4/0AX4XfWh_test_code&scope=calendar HTTP/1.1\r\n";
        let code = extract_code_from_request(line);
        assert_eq!(code.as_deref(), Some("4/0AX4XfWh_test_code"));
    }

    #[test]
    fn extract_code_from_request_no_code() {
        let line = "GET /?error=access_denied HTTP/1.1\r\n";
        let code = extract_code_from_request(line);
        assert!(code.is_none());
    }

    #[test]
    fn parse_event_preserves_raw_location_and_description() {
        let json = serde_json::json!({
            "id": "raw1",
            "summary": "Test",
            "start": { "dateTime": "2026-04-02T10:00:00+02:00" },
            "end": { "dateTime": "2026-04-02T11:00:00+02:00" },
            "location": "Conference Room A",
            "description": "Discuss Q2 plans"
        });
        let meeting = parse_event(&json).expect("should parse event");
        assert_eq!(meeting.raw_location.as_deref(), Some("Conference Room A"));
        assert_eq!(meeting.raw_description.as_deref(), Some("Discuss Q2 plans"));
        // No conference URL since neither field contains a URL
        assert!(meeting.conference_url.is_none());
    }
}
