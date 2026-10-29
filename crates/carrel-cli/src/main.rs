//! `carrel`: the command-line tool for Carrel archives.
//!
//! Every command prints a short report for people, or with `--json` one JSON value for
//! scripts. Errors go to standard error (as `{"error": "…"}` with `--json`) and exit with
//! status 1; usage mistakes exit with status 2.

mod commands;
mod config;
mod providers;
mod review;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// A private, on-device archivist for your files.
#[derive(Debug, Parser)]
#[command(name = "carrel", version = VERSION.as_str(), about)]
struct Cli {
    /// The archive to use. Defaults to the archive containing the current folder, then the
    /// one chosen with `carrel open`.
    #[arg(long, global = true, value_name = "PATH")]
    archive: Option<PathBuf>,

    /// Print one JSON value instead of a report.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create an archive and start its history, or start history for an existing archive
    /// that has none.
    Init {
        /// The folder to create it in; created if missing. Defaults to the current folder.
        path: Option<PathBuf>,
        /// The archive's name. Defaults to the folder's name.
        #[arg(long)]
        name: Option<String>,
        /// Create it even if the folder already has files; they become uncataloged.
        #[arg(long)]
        allow_existing_files: bool,
    },
    /// Choose the archive commands use when run outside one, or show the current choice.
    Open {
        /// The archive, or any folder inside it.
        path: Option<PathBuf>,
    },
    /// Summarize the archive: contents, problems, history and who is writing.
    Status,
    /// Show an item's card.
    Show {
        /// The item's identifier, or the path of the item or its card.
        item: String,
    },
    /// List recorded changes, newest first.
    Log {
        /// Show at most this many.
        #[arg(short = 'n', long, value_name = "COUNT")]
        max_count: Option<usize>,
    },
    /// List the providers on this device (text models, text extraction) and which are used.
    Providers,
    /// Review proposals to file items: list them, show one, or reject them.
    Review {
        /// Include proposals already decided (with no subcommand).
        #[arg(long)]
        all: bool,
        #[command(subcommand)]
        action: Option<ReviewAction>,
    },
}

#[derive(Debug, Subcommand)]
enum ReviewAction {
    /// List proposals waiting for review, by destination.
    List {
        /// Include proposals already decided.
        #[arg(long)]
        all: bool,
    },
    /// Show a proposal: the card it would write, where it would go and why.
    Show {
        /// The proposal's identifier, or its start or end.
        id: String,
    },
    /// Reject proposals; their items stay where they are.
    Reject {
        /// The proposals' identifiers, or their starts or ends.
        #[arg(required = true)]
        ids: Vec<String>,
    },
}

/// The version line, including the archive format this build writes.
static VERSION: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    format!(
        "{} (archive format {})",
        carrel_core::CARREL_VERSION,
        carrel_core::FORMAT_VERSION
    )
});

/// A command's result: a report for people and the same information as JSON.
pub struct Report {
    text: String,
    json: serde_json::Value,
}

impl Report {
    fn new(text: impl Into<String>, json: serde_json::Value) -> Self {
        Self {
            text: text.into(),
            json,
        }
    }
}

/// Why a command failed, in words for the person running it.
#[derive(Debug)]
pub struct Failure(String);

impl Failure {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    fn io(path: &Path, error: &std::io::Error) -> Self {
        Self(format!("{}: {error}", path.display()))
    }
}

impl<E: std::error::Error> From<E> for Failure {
    fn from(error: E) -> Self {
        Self(error.to_string())
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Init {
            path,
            name,
            allow_existing_files,
        } => commands::init(
            path.as_deref().or(cli.archive.as_deref()),
            name,
            allow_existing_files,
        ),
        Command::Open { path } => commands::open(path.as_deref().or(cli.archive.as_deref())),
        Command::Status => commands::status(cli.archive.as_deref()),
        Command::Show { item } => commands::show(cli.archive.as_deref(), &item),
        Command::Log { max_count } => commands::log(cli.archive.as_deref(), max_count),
        Command::Providers => Ok(providers::list()),
        Command::Review { all, action } => match action.unwrap_or(ReviewAction::List { all }) {
            ReviewAction::List { all } => review::list(cli.archive.as_deref(), all),
            ReviewAction::Show { id } => review::show(cli.archive.as_deref(), &id),
            ReviewAction::Reject { ids } => review::reject(cli.archive.as_deref(), &ids),
        },
    };
    match result {
        Ok(report) => {
            if cli.json {
                println!("{:#}", report.json);
            } else {
                print!("{}", report.text);
            }
            ExitCode::SUCCESS
        }
        Err(Failure(message)) => {
            if cli.json {
                eprintln!("{}", serde_json::json!({ "error": message }));
            } else {
                eprintln!("carrel: {message}");
            }
            ExitCode::FAILURE
        }
    }
}
