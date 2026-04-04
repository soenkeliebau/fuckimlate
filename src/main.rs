mod calendar;
mod cli;
mod config;
mod extract;
mod handlers;
mod model;
mod storage;
mod ui;

use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use clap::Parser;
use snafu::{ResultExt, Snafu};
use tracing::info;

/// Top-level error type that wraps all module-specific errors.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
    /// A configuration error occurred.
    #[snafu(display("Configuration error"))]
    Config {
        /// The underlying config module error.
        source: config::Error,
    },

    /// A storage error occurred.
    #[snafu(display("Storage error"))]
    Storage {
        /// The underlying storage module error.
        source: storage::Error,
    },

    /// A calendar API error occurred.
    #[snafu(display("Calendar API error"))]
    Calendar {
        /// The underlying calendar module error.
        source: calendar::Error,
    },

    /// A handler error occurred.
    #[snafu(display("Handler error"))]
    Handler {
        /// The underlying handlers module error.
        source: handlers::Error,
    },

    /// A UI error occurred.
    #[snafu(display("UI error"))]
    Ui {
        /// The underlying UI module error.
        source: ui::Error,
    },
}

/// Result type alias for the top-level application.
type Result<T> = std::result::Result<T, Error>;

fn main() {
    let cli = cli::Cli::parse();

    let filter = match cli.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    tracing_subscriber::fmt().with_env_filter(filter).init();

    if let Err(e) = run(cli) {
        eprintln!("Error: {e}");
        let mut source = std::error::Error::source(&e);
        while let Some(s) = source {
            eprintln!("  Caused by: {s}");
            source = std::error::Error::source(s);
        }
        std::process::exit(1);
    }
}

/// Runs the application logic after CLI parsing and logging setup.
fn run(cli: cli::Cli) -> Result<()> {
    let config = match &cli.config {
        Some(path) => config::Config::load(path).context(ConfigSnafu)?,
        None => config::Config::load_default().context(ConfigSnafu)?,
    };

    let storage_path = config.sync.storage_path.clone().unwrap_or_else(|| {
        let data_dir = dirs::data_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        data_dir.join("fuckimlate").join("meetings.db")
    });

    // Ensure the parent directory exists (best-effort).
    if let Some(parent) = storage_path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::debug!(error = %e, path = %parent.display(), "Failed to create data directory");
    }

    let command = cli.command.unwrap_or(cli::Command::Pick);

    match command {
        cli::Command::Sync => cmd_sync(&config, &storage_path)?,
        cli::Command::Pick => cmd_pick(&config, &storage_path)?,
        cli::Command::Now => cmd_now(&config, &storage_path)?,
        cli::Command::Config => match toml::to_string_pretty(&config) {
            Ok(s) => println!("{s}"),
            Err(e) => eprintln!("Failed to serialize config: {e}"),
        },
    }

    Ok(())
}

/// Fetches today's events from Google Calendar and updates local storage.
fn cmd_sync(config: &config::Config, storage_path: &Path) -> Result<()> {
    info!("Syncing calendar events...");
    let access_token = calendar::get_access_token(&config.calendar).context(CalendarSnafu)?;
    let client = calendar::CalendarClient::new(access_token);
    let meetings = client
        .fetch_all_calendars(&config.calendar.calendar_ids)
        .context(CalendarSnafu)?;
    info!("Fetched {} meetings", meetings.len());

    let store = storage::Storage::open(storage_path).context(StorageSnafu)?;
    store.replace_today(&meetings).context(StorageSnafu)?;
    store.record_sync_time().context(StorageSnafu)?;
    info!("Sync complete");
    Ok(())
}

/// Auto-syncs calendar data if the local cache is stale.
fn maybe_sync(config: &config::Config, store: &storage::Storage) -> Result<()> {
    if store
        .is_stale(config.sync.stale_threshold_minutes)
        .context(StorageSnafu)?
    {
        info!("Data is stale, syncing...");
        let access_token = calendar::get_access_token(&config.calendar).context(CalendarSnafu)?;
        let client = calendar::CalendarClient::new(access_token);
        let meetings = client
            .fetch_all_calendars(&config.calendar.calendar_ids)
            .context(CalendarSnafu)?;
        store.replace_today(&meetings).context(StorageSnafu)?;
        store.record_sync_time().context(StorageSnafu)?;
        info!("Auto-sync complete");
    }
    Ok(())
}

/// Shows today's meetings in fuzzel and dials into the selected one.
fn cmd_pick(config: &config::Config, storage_path: &Path) -> Result<()> {
    let store = storage::Storage::open(storage_path).context(StorageSnafu)?;
    maybe_sync(config, &store)?;

    let meetings = store.meetings_today().context(StorageSnafu)?;
    if meetings.is_empty() {
        if let Err(e) = handlers::notify_error("No meetings today") {
            tracing::debug!(error = %e, "Failed to send desktop notification");
        }
        return Ok(());
    }

    match ui::pick_meeting(&meetings, &config.ui).context(UiSnafu)? {
        Some(meeting) => {
            let result = handlers::launch_meeting(meeting, config);
            if let Err(ref e) = result {
                // Show a desktop notification so the user sees the error even
                // when running via fuzzel (where stderr is not visible).
                if let Err(notify_err) = handlers::notify_error(&e.to_string()) {
                    tracing::debug!(error = %notify_err, "Failed to send desktop notification");
                }
            }
            result.context(HandlerSnafu)?;
        }
        None => {
            // User dismissed fuzzel without selecting a meeting.
        }
    }
    Ok(())
}

/// If there's exactly one ongoing-but-ending-soon meeting and exactly one
/// upcoming meeting, return only the upcoming one. Otherwise return the
/// list unchanged.
fn prefer_upcoming_if_transitioning(
    meetings: Vec<&model::Meeting>,
    now: DateTime<Local>,
    lookahead: chrono::Duration,
) -> Vec<&model::Meeting> {
    if meetings.len() != 2 {
        return meetings;
    }
    let ending_soon: Vec<_> = meetings
        .iter()
        .filter(|m| m.start_time <= now && m.end_time > now && m.end_time - now <= lookahead)
        .collect();
    let upcoming: Vec<_> = meetings.iter().filter(|m| m.start_time > now).collect();
    if ending_soon.len() == 1 && upcoming.len() == 1 {
        vec![*upcoming[0]]
    } else {
        meetings
    }
}

/// Auto-dials into the current or imminent meeting.
fn cmd_now(config: &config::Config, storage_path: &Path) -> Result<()> {
    let store = storage::Storage::open(storage_path).context(StorageSnafu)?;
    maybe_sync(config, &store)?;

    let meetings = store.meetings_today().context(StorageSnafu)?;
    let now = chrono::Local::now();

    let lookback_mins = i64::try_from(config.panic_mode.lookback_minutes).unwrap_or(i64::MAX);
    let lookahead_mins = i64::try_from(config.panic_mode.lookahead_minutes).unwrap_or(i64::MAX);
    let lookback = chrono::Duration::minutes(lookback_mins);
    let lookahead = chrono::Duration::minutes(lookahead_mins);

    let relevant: Vec<&model::Meeting> = meetings
        .iter()
        .filter(|m| {
            let ongoing = m.start_time <= now && m.end_time > now;
            let started_recently = m.start_time <= now && now - m.start_time <= lookback;
            let starting_soon = m.start_time > now && m.start_time - now <= lookahead;
            ongoing || started_recently || starting_soon
        })
        .collect();

    // When an ongoing meeting is ending soon and an upcoming meeting exists,
    // prefer the upcoming one — the user likely wants to dial into the next call.
    let relevant = prefer_upcoming_if_transitioning(relevant, now, lookahead);

    match relevant.len() {
        0 => {
            if let Err(e) = handlers::notify_error("No meetings right now") {
                tracing::debug!(error = %e, "Failed to send desktop notification");
            }
        }
        1 => {
            handlers::launch_meeting(relevant[0], config).context(HandlerSnafu)?;
        }
        _ => {
            // Multiple matches — show fuzzel with just these.
            let owned: Vec<model::Meeting> = relevant.into_iter().cloned().collect();
            if let Some(meeting) = ui::pick_meeting(&owned, &config.ui).context(UiSnafu)? {
                handlers::launch_meeting(meeting, config).context(HandlerSnafu)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn make_meeting(title: &str, start: DateTime<Local>, end: DateTime<Local>) -> model::Meeting {
        model::Meeting {
            id: title.to_owned(),
            title: title.to_owned(),
            start_time: start,
            end_time: end,
            conference_url: Some("https://example.com/meet".to_owned()),
            conference_type: Some(model::ConferenceType::Zoom),
            raw_location: None,
            raw_description: None,
        }
    }

    #[test]
    fn transition_prefers_upcoming_meeting() {
        let now = Local.with_ymd_and_hms(2026, 4, 4, 10, 57, 0).unwrap();
        let lookahead = Duration::minutes(5);

        let current = make_meeting(
            "Current",
            now - Duration::minutes(57),
            now + Duration::minutes(3),
        );
        let upcoming = make_meeting(
            "Upcoming",
            now + Duration::minutes(3),
            now + Duration::minutes(63),
        );

        let result = prefer_upcoming_if_transitioning(vec![&current, &upcoming], now, lookahead);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Upcoming");
    }

    #[test]
    fn no_transition_when_current_not_ending_soon() {
        let now = Local.with_ymd_and_hms(2026, 4, 4, 10, 40, 0).unwrap();
        let lookahead = Duration::minutes(5);

        let current = make_meeting(
            "Current",
            now - Duration::minutes(40),
            now + Duration::minutes(20),
        );
        let upcoming = make_meeting(
            "Upcoming",
            now + Duration::minutes(3),
            now + Duration::minutes(63),
        );

        let result = prefer_upcoming_if_transitioning(vec![&current, &upcoming], now, lookahead);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn single_ongoing_unchanged() {
        let now = Local.with_ymd_and_hms(2026, 4, 4, 10, 30, 0).unwrap();
        let lookahead = Duration::minutes(5);

        let current = make_meeting(
            "Current",
            now - Duration::minutes(30),
            now + Duration::minutes(30),
        );

        let result = prefer_upcoming_if_transitioning(vec![&current], now, lookahead);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Current");
    }

    #[test]
    fn single_upcoming_unchanged() {
        let now = Local.with_ymd_and_hms(2026, 4, 4, 10, 57, 0).unwrap();
        let lookahead = Duration::minutes(5);

        let upcoming = make_meeting(
            "Upcoming",
            now + Duration::minutes(3),
            now + Duration::minutes(63),
        );

        let result = prefer_upcoming_if_transitioning(vec![&upcoming], now, lookahead);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Upcoming");
    }
}
