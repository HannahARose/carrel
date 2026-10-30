//! `carrel`: the command-line tool for Carrel archives.
//!
//! Every command prints a short report for people, or with `--json` one JSON value for
//! scripts. Errors go to standard error (as `{"error": "…"}` with `--json`) and exit with
//! status 1; usage mistakes exit with status 2.

mod backup;
mod commands;
mod config;
mod ingest;
mod providers;
mod questions;
mod review;
mod tui;

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
    command: Option<Command>,
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
    /// Propose filing files and folders: each gets a drafted card and a destination, saved
    /// as a proposal for `carrel review`. Nothing is moved until a proposal is approved.
    Ingest {
        /// The files and folders. Each is one item.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Ingest each entry inside a folder as its own item.
        #[arg(long, conflicts_with = "whole")]
        each: bool,
        /// Ingest a folder as one item even when its name suggests a pile, such as Downloads.
        #[arg(long)]
        whole: bool,
        /// Show what would be proposed without saving anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Review proposals to file items: list, show, edit, approve, hold, move or reject them.
    Review {
        /// Include proposals already decided (with no subcommand).
        #[arg(long)]
        all: bool,
        #[command(subcommand)]
        action: Option<ReviewAction>,
    },
    /// Open the interactive mode: status, review, questions, browsing and history in one
    /// screen. `carrel` with no command opens it too, when run in a terminal.
    Tui,
    /// Undo a recorded change with a new change that reverses it; see `carrel log`.
    Undo {
        /// The change's identifier, or its start.
        change: String,
    },
    /// Back up the archive's contents with Kopia: set up, snapshot, list, verify or restore.
    Backup {
        #[command(subcommand)]
        action: BackupAction,
    },
    /// Questions ingest needs you to answer, most items first.
    Questions {
        #[command(subcommand)]
        action: Option<QuestionsAction>,
    },
}

#[derive(Debug, Subcommand)]
enum BackupAction {
    /// Create a backup repository at PATH, or connect to the one there. An external drive is
    /// safest. The password is asked for once and kept in this device's keychain.
    Setup {
        /// Where the repository is or will be.
        path: PathBuf,
        /// A name for it, such as "Backup drive".
        #[arg(long)]
        description: Option<String>,
    },
    /// Take a snapshot now.
    Now,
    /// List the snapshots, newest first, with the change each was taken with.
    List,
    /// Check the backups, reading back a share of the files.
    Verify {
        /// How much of the files' contents to read back, in percent.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(0..=100))]
        percent: u8,
    },
    /// Restore an item's contents as they were, into a new folder beside the archive.
    Restore {
        /// The item: its identifier, or its card's or contents' path.
        item: String,
        /// The change to restore it as of (a history commit, or its start).
        #[arg(long)]
        at: Option<String>,
        /// Where to restore it; it must not exist yet.
        #[arg(long)]
        to: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum QuestionsAction {
    /// List the questions waiting for an answer.
    List,
    /// Answer a question: what a term means, or which collection its items belong in. Items
    /// waiting on a term are drafted again with the answer.
    Answer {
        /// The question's identifier, or its start or end.
        id: String,
        /// The answer: a sentence for a term, or a collection's path.
        #[arg(required = true, num_args = 1..)]
        answer: Vec<String>,
    },
    /// Dismiss a question; its items stop waiting on it.
    Dismiss {
        /// The question's identifier, or its start or end.
        id: String,
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
    /// Approve proposals: file the items where proposed, as one history commit.
    Approve {
        /// The proposals' identifiers, or their starts or ends.
        #[arg(required_unless_present = "all_high", conflicts_with = "all_high")]
        ids: Vec<String>,
        /// Approve every proposal with high confidence that isn't sensitive or a duplicate.
        #[arg(long)]
        all_high: bool,
    },
    /// File proposals' items in Holding, to be placed later.
    Hold {
        /// The proposals' identifiers, or their starts or ends.
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Edit a proposal's card in your editor ($VISUAL or $EDITOR) before approving it.
    Edit {
        /// The proposal's identifier, or its start or end.
        id: String,
    },
    /// Propose another collection for an item. It's filed there when approved.
    MoveTo {
        /// The proposal's identifier, or its start or end.
        id: String,
        /// The collection's path, such as `Projects/Hardware`.
        collection: String,
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
    let Some(command) = cli.command else {
        use std::io::IsTerminal as _;
        if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() && !cli.json {
            return finish(tui::run(cli.archive.as_deref()), false);
        }
        // Not in a terminal, as in scripts: the usage message, as for any missing command.
        <Cli as clap::CommandFactory>::command()
            .error(
                clap::error::ErrorKind::MissingSubcommand,
                "a command is needed when not in a terminal",
            )
            .exit();
    };
    let result = match command {
        Command::Tui => tui::run(cli.archive.as_deref()),
        Command::Undo { change } => commands::undo(cli.archive.as_deref(), &change),
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
        Command::Ingest {
            paths,
            each,
            whole,
            dry_run,
        } => ingest::ingest(
            cli.archive.as_deref(),
            &paths,
            carrel_core::ingest::IntakeOptions { each, whole },
            dry_run,
        ),
        Command::Review { all, action } => match action.unwrap_or(ReviewAction::List { all }) {
            ReviewAction::List { all } => review::list(cli.archive.as_deref(), all),
            ReviewAction::Show { id } => review::show(cli.archive.as_deref(), &id),
            ReviewAction::Reject { ids } => review::reject(cli.archive.as_deref(), &ids),
            ReviewAction::Approve { ids, all_high } => {
                review::approve(cli.archive.as_deref(), &ids, all_high)
            }
            ReviewAction::Hold { ids } => review::hold(cli.archive.as_deref(), &ids),
            ReviewAction::Edit { id } => review::edit(cli.archive.as_deref(), &id),
            ReviewAction::MoveTo { id, collection } => {
                review::move_to(cli.archive.as_deref(), &id, &collection)
            }
        },
        Command::Backup { action } => match action {
            BackupAction::Setup { path, description } => {
                backup::setup(cli.archive.as_deref(), &path, description.as_deref())
            }
            BackupAction::Now => backup::now(cli.archive.as_deref()),
            BackupAction::List => backup::list(cli.archive.as_deref()),
            BackupAction::Verify { percent } => backup::verify(cli.archive.as_deref(), percent),
            BackupAction::Restore { item, at, to } => {
                backup::restore(cli.archive.as_deref(), &item, at.as_deref(), to.as_deref())
            }
        },
        Command::Questions { action } => match action.unwrap_or(QuestionsAction::List) {
            QuestionsAction::List => questions::list(cli.archive.as_deref()),
            QuestionsAction::Answer { id, answer } => {
                questions::answer(cli.archive.as_deref(), &id, &answer)
            }
            QuestionsAction::Dismiss { id } => questions::dismiss(cli.archive.as_deref(), &id),
        },
    };
    finish(result, cli.json)
}

/// Prints a command's result and gives its exit status.
fn finish(result: Result<Report, Failure>, json: bool) -> ExitCode {
    match result {
        Ok(report) => {
            if json {
                println!("{:#}", report.json);
            } else {
                print!("{}", report.text);
            }
            ExitCode::SUCCESS
        }
        Err(Failure(message)) => {
            if json {
                eprintln!("{}", serde_json::json!({ "error": message }));
            } else {
                eprintln!("carrel: {message}");
            }
            ExitCode::FAILURE
        }
    }
}
