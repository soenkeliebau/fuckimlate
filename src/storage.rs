// SQLite-backed storage for meeting data and sync metadata.

use std::path::Path;
use std::str::FromStr;

use chrono::{DateTime, Local, NaiveTime, TimeZone, Utc};
use rusqlite::{Connection, params};
use snafu::{ResultExt, Snafu};

use crate::model::{ConferenceType, Meeting};

/// Error type for the storage module.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
    /// Failed to open the SQLite database.
    #[snafu(display("Failed to open database at {path}"))]
    OpenDatabase {
        /// The path that was used to open the database.
        path: String,
        /// The underlying SQLite error.
        source: rusqlite::Error,
    },

    /// Failed to open an in-memory SQLite database.
    #[snafu(display("Failed to open in-memory database"))]
    OpenInMemoryDatabase {
        /// The underlying SQLite error.
        source: rusqlite::Error,
    },

    /// Failed to create the database schema.
    #[snafu(display("Failed to create database schema"))]
    CreateSchema {
        /// The underlying SQLite error.
        source: rusqlite::Error,
    },

    /// Failed to insert a meeting into the database.
    #[snafu(display("Failed to insert meeting with id {id}"))]
    InsertMeeting {
        /// The meeting ID that failed to insert.
        id: String,
        /// The underlying SQLite error.
        source: rusqlite::Error,
    },

    /// Failed to delete meetings from the database.
    #[snafu(display("Failed to delete meetings"))]
    DeleteMeetings {
        /// The underlying SQLite error.
        source: rusqlite::Error,
    },

    /// Failed to query meetings from the database.
    #[snafu(display("Failed to query meetings"))]
    QueryMeetings {
        /// The underlying SQLite error.
        source: rusqlite::Error,
    },

    /// Failed to record sync time.
    #[snafu(display("Failed to record sync time"))]
    RecordSyncTime {
        /// The underlying SQLite error.
        source: rusqlite::Error,
    },

    /// Failed to check staleness.
    #[snafu(display("Failed to check staleness"))]
    CheckStaleness {
        /// The underlying SQLite error.
        source: rusqlite::Error,
    },

    /// Failed to parse a stored date-time value.
    #[snafu(display("Failed to parse date-time '{value}'"))]
    ParseDateTime {
        /// The string value that could not be parsed.
        value: String,
        /// The underlying chrono parse error.
        source: chrono::ParseError,
    },

    /// A local date-time could not be resolved unambiguously (e.g. DST gap/overlap).
    #[snafu(display("Ambiguous or invalid local date-time: {detail}"))]
    AmbiguousLocalDateTime {
        /// Description of the ambiguous value.
        detail: String,
    },

    /// Failed to begin a database transaction.
    #[snafu(display("Failed to begin database transaction"))]
    BeginTransaction {
        /// The underlying SQLite error.
        source: rusqlite::Error,
    },

    /// Failed to commit a database transaction.
    #[snafu(display("Failed to commit database transaction"))]
    CommitTransaction {
        /// The underlying SQLite error.
        source: rusqlite::Error,
    },
}

/// Result type alias for the storage module.
pub type Result<T> = std::result::Result<T, Error>;

/// SQL schema for the meetings table.
const SCHEMA_MEETINGS: &str = "
CREATE TABLE IF NOT EXISTS meetings (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    start_time TEXT NOT NULL,
    end_time TEXT NOT NULL,
    conference_url TEXT,
    conference_type TEXT,
    raw_location TEXT,
    raw_description TEXT
);
";

/// SQL schema for the sync metadata table.
const SCHEMA_SYNC_META: &str = "
CREATE TABLE IF NOT EXISTS sync_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

/// SQLite-backed storage for persisting meeting data and sync metadata.
///
/// Meetings are stored with their start and end times as RFC 3339 strings.
/// The storage provides methods to replace today's meetings atomically,
/// query today's meetings sorted by start time, and track when the last
/// sync occurred.
pub struct Storage {
    conn: Connection,
}

impl Storage {
    /// Opens (or creates) a SQLite database at the given path and initializes the schema.
    ///
    /// # Errors
    ///
    /// Returns [`Error::OpenDatabase`] if the database file cannot be opened or created.
    /// Returns [`Error::CreateSchema`] if the schema creation statements fail.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path_str = path.as_ref().display().to_string();
        let conn =
            Connection::open(path.as_ref()).context(OpenDatabaseSnafu { path: &path_str })?;
        let store = Self { conn };
        store.create_schema()?;
        Ok(store)
    }

    /// Opens an in-memory SQLite database and initializes the schema.
    ///
    /// This is primarily useful for testing.
    ///
    /// # Errors
    ///
    /// Returns [`Error::OpenInMemoryDatabase`] if the in-memory database cannot be created.
    /// Returns [`Error::CreateSchema`] if the schema creation statements fail.
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context(OpenInMemoryDatabaseSnafu)?;
        let store = Self { conn };
        store.create_schema()?;
        Ok(store)
    }

    /// Creates the database schema if it does not already exist.
    fn create_schema(&self) -> Result<()> {
        self.conn
            .execute_batch(SCHEMA_MEETINGS)
            .context(CreateSchemaSnafu)?;
        self.conn
            .execute_batch(SCHEMA_SYNC_META)
            .context(CreateSchemaSnafu)?;
        Ok(())
    }

    /// Replaces all meetings for today with the given list.
    ///
    /// This operation is atomic: it deletes all existing meetings whose start time
    /// falls on today's date and inserts the new meetings within a single transaction.
    /// All timestamps are stored as UTC RFC 3339 strings so that lexicographic
    /// comparison in SQLite works correctly regardless of timezone offset.
    ///
    /// # Errors
    ///
    /// Returns [`Error::AmbiguousLocalDateTime`] if today's boundaries cannot be computed.
    /// Returns [`Error::BeginTransaction`] if the transaction cannot be started.
    /// Returns [`Error::DeleteMeetings`] if the delete operation fails.
    /// Returns [`Error::InsertMeeting`] if any meeting insertion fails.
    /// Returns [`Error::CommitTransaction`] if the transaction cannot be committed.
    pub fn replace_today(&self, meetings: &[Meeting]) -> Result<()> {
        let (today_start, today_end) = today_boundaries()?;
        let start_str = today_start.to_rfc3339();
        let end_str = today_end.to_rfc3339();

        // rusqlite requires `&mut self` for transaction(), so we use execute + manual commit
        self.conn
            .execute_batch("BEGIN")
            .context(BeginTransactionSnafu)?;

        let delete_result = self.conn.execute(
            "DELETE FROM meetings WHERE start_time >= ?1 AND start_time <= ?2",
            params![start_str, end_str],
        );

        if let Err(e) = delete_result {
            // Best-effort rollback; ignore rollback errors since the original error matters
            let _ = self.conn.execute_batch("ROLLBACK");
            return Err(Error::DeleteMeetings { source: e });
        }

        for meeting in meetings {
            let start_utc = meeting.start_time.with_timezone(&Utc).to_rfc3339();
            let end_utc = meeting.end_time.with_timezone(&Utc).to_rfc3339();
            let insert_result = self.conn.execute(
                "INSERT OR REPLACE INTO meetings (id, title, start_time, end_time, conference_url, conference_type, raw_location, raw_description) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    meeting.id,
                    meeting.title,
                    start_utc,
                    end_utc,
                    meeting.conference_url,
                    meeting.conference_type.map(|ct| ct.to_string()),
                    meeting.raw_location,
                    meeting.raw_description,
                ],
            );

            if let Err(e) = insert_result {
                let _ = self.conn.execute_batch("ROLLBACK");
                return Err(Error::InsertMeeting {
                    id: meeting.id.clone(),
                    source: e,
                });
            }
        }

        self.conn
            .execute_batch("COMMIT")
            .context(CommitTransactionSnafu)?;

        Ok(())
    }

    /// Returns all meetings for today, sorted by start time in ascending order.
    ///
    /// Timestamps are stored as UTC RFC 3339 strings and are converted back to
    /// local time on retrieval.
    ///
    /// # Errors
    ///
    /// Returns [`Error::AmbiguousLocalDateTime`] if today's boundaries cannot be computed.
    /// Returns [`Error::QueryMeetings`] if the database query fails.
    /// Returns [`Error::ParseDateTime`] if a stored date-time value cannot be parsed.
    pub fn meetings_today(&self) -> Result<Vec<Meeting>> {
        let (today_start, today_end) = today_boundaries()?;
        let start_str = today_start.to_rfc3339();
        let end_str = today_end.to_rfc3339();

        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, title, start_time, end_time, conference_url, conference_type, raw_location, raw_description FROM meetings WHERE start_time >= ?1 AND start_time <= ?2 ORDER BY start_time ASC",
            )
            .context(QueryMeetingsSnafu)?;

        let rows = stmt
            .query_map(params![start_str, end_str], |row| {
                Ok(MeetingRow {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    start_time: row.get(2)?,
                    end_time: row.get(3)?,
                    conference_url: row.get(4)?,
                    conference_type: row.get(5)?,
                    raw_location: row.get(6)?,
                    raw_description: row.get(7)?,
                })
            })
            .context(QueryMeetingsSnafu)?;

        let mut meetings = Vec::new();
        for row_result in rows {
            let row = row_result.context(QueryMeetingsSnafu)?;
            let meeting = row_to_meeting(row)?;
            meetings.push(meeting);
        }

        Ok(meetings)
    }

    /// Records the current time as the last sync time.
    ///
    /// # Errors
    ///
    /// Returns [`Error::RecordSyncTime`] if the upsert operation fails.
    pub fn record_sync_time(&self) -> Result<()> {
        let now = Local::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO sync_meta (key, value) VALUES ('last_sync_time', ?1)",
                params![now],
            )
            .context(RecordSyncTimeSnafu)?;
        Ok(())
    }

    /// Checks whether the stored sync time is stale.
    ///
    /// Returns `true` if no sync time has been recorded or if the last sync time
    /// is older than `threshold_minutes` minutes ago.
    ///
    /// # Errors
    ///
    /// Returns [`Error::CheckStaleness`] if the database query fails.
    /// Returns [`Error::ParseDateTime`] if the stored sync time cannot be parsed.
    pub fn is_stale(&self, threshold_minutes: u64) -> Result<bool> {
        let result: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM sync_meta WHERE key = 'last_sync_time'",
                [],
                |row| row.get(0),
            )
            .optional()
            .context(CheckStalenessSnafu)?;

        match result {
            None => Ok(true),
            Some(value) => {
                let last_sync = DateTime::parse_from_rfc3339(&value)
                    .context(ParseDateTimeSnafu { value: &value })?
                    .with_timezone(&Local);

                let elapsed = Local::now().signed_duration_since(last_sync);
                let threshold_mins = i64::try_from(threshold_minutes).unwrap_or(i64::MAX);
                let threshold = chrono::Duration::minutes(threshold_mins);
                Ok(elapsed > threshold)
            }
        }
    }
}

/// An intermediate row type for reading meetings from SQLite before conversion.
struct MeetingRow {
    id: String,
    title: String,
    start_time: String,
    end_time: String,
    conference_url: Option<String>,
    conference_type: Option<String>,
    raw_location: Option<String>,
    raw_description: Option<String>,
}

/// Converts a raw database row into a [`Meeting`].
///
/// Timestamps stored as UTC RFC 3339 strings are parsed and converted back to
/// the local timezone.
///
/// # Errors
///
/// Returns [`Error::ParseDateTime`] if the start or end time cannot be parsed from RFC 3339.
fn row_to_meeting(row: MeetingRow) -> Result<Meeting> {
    let start_time = parse_rfc3339_local(&row.start_time)?;
    let end_time = parse_rfc3339_local(&row.end_time)?;

    let conference_type = match row
        .conference_type
        .as_deref()
        .map(ConferenceType::from_str)
        .transpose()
    {
        Ok(ct) => ct,
        Err(e) => {
            tracing::warn!(error = %e, "Unknown conference type in database, using None");
            None
        }
    };

    Ok(Meeting {
        id: row.id,
        title: row.title,
        start_time,
        end_time,
        conference_url: row.conference_url,
        conference_type,
        raw_location: row.raw_location,
        raw_description: row.raw_description,
    })
}

/// Parses an RFC 3339 string into a `DateTime<Local>`.
///
/// # Errors
///
/// Returns [`Error::ParseDateTime`] if the value cannot be parsed.
fn parse_rfc3339_local(value: &str) -> Result<DateTime<Local>> {
    let parsed = DateTime::parse_from_rfc3339(value).context(ParseDateTimeSnafu { value })?;
    Ok(parsed.with_timezone(&Local))
}

/// Returns the start and end boundaries of today as UTC `DateTime<Utc>`.
///
/// Computes the local-time boundaries (midnight and 23:59:59), converts them
/// to UTC, and returns them as UTC timestamps. This ensures that RFC 3339
/// string comparisons in SQLite work correctly regardless of timezone offset.
///
/// # Errors
///
/// Returns [`Error::AmbiguousLocalDateTime`] if midnight or end-of-day cannot
/// be resolved unambiguously in the local timezone (e.g. during a DST transition).
fn today_boundaries() -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    let today = Local::now().date_naive();

    // These NaiveTime values are hardcoded valid times, so `from_hms_opt`
    // will always return `Some`.
    let start_of_day =
        NaiveTime::from_hms_opt(0, 0, 0).ok_or_else(|| Error::AmbiguousLocalDateTime {
            detail: "00:00:00 could not be constructed".to_owned(),
        })?;
    let end_of_day =
        NaiveTime::from_hms_opt(23, 59, 59).ok_or_else(|| Error::AmbiguousLocalDateTime {
            detail: "23:59:59 could not be constructed".to_owned(),
        })?;

    let start = today.and_time(start_of_day);
    let end = today.and_time(end_of_day);

    let local_start = Local
        .from_local_datetime(&start)
        .earliest()
        .ok_or_else(|| Error::AmbiguousLocalDateTime {
            detail: format!("midnight on {today} is ambiguous or invalid in local timezone"),
        })?;
    let local_end = Local.from_local_datetime(&end).earliest().ok_or_else(|| {
        Error::AmbiguousLocalDateTime {
            detail: format!("23:59:59 on {today} is ambiguous or invalid in local timezone"),
        }
    })?;

    Ok((
        local_start.with_timezone(&Utc),
        local_end.with_timezone(&Utc),
    ))
}

/// Extension trait to make `rusqlite::OptionalExtension` available.
///
/// This provides `.optional()` on `Result<T, rusqlite::Error>` to convert
/// `QueryReturnedNoRows` into `Ok(None)`.
trait OptionalExt<T> {
    /// Converts a `QueryReturnedNoRows` error into `Ok(None)`.
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for std::result::Result<T, rusqlite::Error> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_meeting(id: &str, title: &str, hour: u32) -> Meeting {
        let today = Local::now().date_naive();
        let start_naive = today
            .and_hms_opt(hour, 0, 0)
            .expect("test hour should be valid");
        let start = Local
            .from_local_datetime(&start_naive)
            .earliest()
            .expect("test start time should be unambiguous");
        let end_naive = today
            .and_hms_opt(hour + 1, 0, 0)
            .expect("test end hour should be valid");
        let end = Local
            .from_local_datetime(&end_naive)
            .earliest()
            .expect("test end time should be unambiguous");
        Meeting {
            id: id.to_owned(),
            title: title.to_owned(),
            start_time: start,
            end_time: end,
            conference_url: Some("https://meet.google.com/abc".to_owned()),
            conference_type: Some(ConferenceType::GoogleMeet),
            raw_location: None,
            raw_description: None,
        }
    }

    #[test]
    fn store_and_retrieve_meetings() {
        let store = Storage::open_in_memory().expect("open in-memory db");
        let meetings = vec![
            make_meeting("1", "Standup", 9),
            make_meeting("2", "Lunch", 12),
        ];
        store.replace_today(&meetings).expect("store meetings");
        let loaded = store.meetings_today().expect("load meetings");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].title, "Standup");
        assert_eq!(loaded[1].title, "Lunch");
    }

    #[test]
    fn replace_today_is_idempotent() {
        let store = Storage::open_in_memory().expect("open in-memory db");
        let meetings = vec![make_meeting("1", "Standup", 9)];
        store.replace_today(&meetings).expect("first store");
        store.replace_today(&meetings).expect("second store");
        let loaded = store.meetings_today().expect("load meetings");
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn staleness_check() {
        let store = Storage::open_in_memory().expect("open in-memory db");
        assert!(
            store.is_stale(60).expect("check staleness"),
            "should be stale with no sync"
        );
        store.record_sync_time().expect("record sync");
        assert!(
            !store.is_stale(60).expect("check staleness"),
            "should not be stale right after sync"
        );
    }
}
