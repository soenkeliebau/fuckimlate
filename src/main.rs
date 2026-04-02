mod calendar;
mod cli;
mod config;
mod extract;
mod handlers;
mod model;
mod storage;
mod ui;

use std::path::{Path, PathBuf};

use clap::Parser;
use snafu::{ResultExt, Snafu};
use tracing::info;

/// Top-level error type that wraps all module-specific errors.
#[derive(Debug, Snafu)]
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
        let data_dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from("~/.local/share"));
        data_dir.join("fuckimlate").join("meetings.db")
    });

    // Ensure the parent directory exists (best-effort).
    if let Some(parent) = storage_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let command = cli.command.unwrap_or(cli::Command::Pick);

    match command {
        cli::Command::Sync => cmd_sync(&config, &storage_path)?,
        cli::Command::Pick => cmd_pick(&config, &storage_path)?,
        cli::Command::Now => cmd_now(&config, &storage_path)?,
        cli::Command::Config => {
            println!("{}", toml::to_string_pretty(&config).unwrap_or_default());
        }
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
        // Best-effort notification; ignore errors.
        let _ = handlers::notify_error("No meetings today");
        return Ok(());
    }

    match ui::pick_meeting(&meetings, &config.ui).context(UiSnafu)? {
        Some(meeting) => {
            handlers::launch_meeting(meeting, config).context(HandlerSnafu)?;
        }
        None => {
            // User dismissed fuzzel without selecting a meeting.
        }
    }
    Ok(())
}

/// Auto-dials into the current or imminent meeting.
fn cmd_now(config: &config::Config, storage_path: &Path) -> Result<()> {
    let store = storage::Storage::open(storage_path).context(StorageSnafu)?;
    maybe_sync(config, &store)?;

    let meetings = store.meetings_today().context(StorageSnafu)?;
    let now = chrono::Local::now();

    let lookback = chrono::Duration::minutes(config.panic_mode.lookback_minutes as i64);
    let lookahead = chrono::Duration::minutes(config.panic_mode.lookahead_minutes as i64);

    let relevant: Vec<&model::Meeting> = meetings
        .iter()
        .filter(|m| {
            let started_recently = m.start_time <= now && now - m.start_time <= lookback;
            let starting_soon = m.start_time > now && m.start_time - now <= lookahead;
            started_recently || starting_soon
        })
        .collect();

    match relevant.len() {
        0 => {
            // Best-effort notification; ignore errors.
            let _ = handlers::notify_error("No meetings right now");
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
