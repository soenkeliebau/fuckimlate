use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Conference call dialer — never be late again.
#[derive(Debug, Parser)]
#[command(name = "fuckimlate", version, about)]
pub struct Cli {
    /// Path to config file.
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Increase log verbosity (-v, -vv, -vvv).
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Subcommand to run. Defaults to `pick`.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Available subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Fetch today's events from Google Calendar and update local storage.
    Sync,
    /// Show today's meetings in fuzzel and dial into the selected one.
    Pick,
    /// Auto-dial into the current or imminent meeting.
    Now,
    /// Print the resolved configuration.
    Config,
}
