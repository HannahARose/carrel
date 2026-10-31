//! `carrel`: the command-line tool for Carrel archives.
//!
//! Every command prints a short report for people, or with `--json` one JSON value for
//! scripts. Errors go to standard error (as `{"error": "…"}` with `--json`) and exit with
//! status 1; usage mistakes exit with status 2.

mod backup;
mod bench;
mod checkout;
mod commands;
mod config;
mod ingest;
mod models;
mod paths;
mod picker;
mod progress;
mod providers;
mod questions;
mod review;
mod settings;
mod tui;
mod worker;

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
    /// Add files and folders to the intake queue. A worker drafts each one's card and
    /// destination in the background, saving proposals for `carrel review` as they're ready.
    /// Nothing is moved until a proposal is approved.
    Ingest(IngestArgs),
    /// Show or change settings: this device's file picker and editor, and the archive's
    /// shared settings, whose changes can be undone.
    Settings {
        #[command(subcommand)]
        action: Option<SettingsAction>,
    },
    /// Draft the items waiting in the intake queue on this device, until none are left.
    /// Adding to the queue starts one in the background, so this is rarely run by hand.
    Worker,
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
    /// Rename an item and its card together, as when `carrel status` finds a name that won't
    /// work on every system. Links to it follow, and the change can be undone.
    Rename {
        /// The item: its identifier, or its card's or contents' path.
        item: String,
        /// The new name; without one, the portable name `carrel status` suggests.
        new_name: Option<String>,
    },
    /// Check an item out to work on: a copy goes to a working folder, and the archive copy is
    /// read-only until it's returned.
    Checkout {
        /// The item: its identifier, or its card's or contents' path.
        item: String,
        /// The folder to put the copy in; the Desktop by default.
        #[arg(long, value_name = "FOLDER")]
        to: Option<PathBuf>,
        /// When it's due back, as YYYY-MM-DD.
        #[arg(long, value_name = "DATE")]
        due: Option<String>,
    },
    /// List what's checked out, where, and whether it has changed.
    Checkouts,
    /// Return a checked-out item: shows what changed and the history entry it adds, asks,
    /// then replaces the archive copy with the working copy.
    Return {
        /// The item: its identifier, or its card's or contents' path.
        item: String,
        /// Where the working copy is now, if it moved.
        #[arg(long, value_name = "PATH")]
        from: Option<PathBuf>,
        /// Return without asking.
        #[arg(long)]
        yes: bool,
        /// Cancel the checkout instead: unlock the archive copy as it was, without the
        /// working copy's changes.
        #[arg(long, conflicts_with = "from")]
        discard: bool,
    },
    /// Create collections: folders with a design document saying what belongs in them.
    Collection {
        #[command(subcommand)]
        action: CollectionAction,
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
    /// Measure text models on the benchmark's test set, for choosing defaults
    /// (`docs/design/benchmark.md`).
    #[command(hide = true)]
    Bench(bench::BenchArgs),
    /// Models for local providers: list them, or download or remove one. Nothing is
    /// downloaded unless asked for.
    Models {
        #[command(subcommand)]
        action: Option<ModelsAction>,
    },
}

#[derive(Debug, Subcommand)]
enum ModelsAction {
    /// List the models Carrel can download, and those already in the models folder.
    List,
    /// Download a model, after showing its size and license and asking. The file is checked
    /// against the checksum Carrel ships with before it's used.
    Download {
        /// The model's name, from `carrel models`.
        name: String,
        /// `gguf` for the llama.cpp provider, or `mlx` for the MLX provider on Apple silicon.
        #[arg(long, value_enum, default_value_t)]
        format: models::Format,
        /// Download without asking.
        #[arg(long)]
        yes: bool,
    },
    /// Delete a downloaded model, after asking.
    Remove {
        /// The model's name.
        name: String,
        /// Which download to delete: `gguf` or `mlx`.
        #[arg(long, value_enum, default_value_t)]
        format: models::Format,
        /// Delete without asking.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, clap::Args)]
#[allow(clippy::struct_excessive_bools)] // each is a command-line flag
struct IngestArgs {
    /// The files and folders. Each is one item.
    paths: Vec<PathBuf>,
    /// Ingest each entry inside a folder as its own item.
    #[arg(long, conflicts_with = "whole")]
    each: bool,
    /// Ingest a folder as one item even when its name suggests a pile, such as Downloads.
    #[arg(long)]
    whole: bool,
    /// Show what would be added, at once, without queueing anything or running a model.
    #[arg(long, conflicts_with = "wait")]
    dry_run: bool,
    /// Stay until the items are drafted, showing progress, and report each.
    #[arg(long)]
    wait: bool,
    /// Show the worker and what's waiting in the queue on this device.
    #[arg(long, conflicts_with_all = ["dry_run", "wait", "remove", "first", "retry"])]
    status: bool,
    /// Take a waiting item off the queue, by its identifier or its start.
    #[arg(long, value_name = "ID", conflicts_with_all = ["first", "retry"])]
    remove: Option<String>,
    /// Move a waiting item to the front of the queue.
    #[arg(long, value_name = "ID", conflicts_with = "retry")]
    first: Option<String>,
    /// Try a failed item again.
    #[arg(long, value_name = "ID")]
    retry: Option<String>,
}

impl IngestArgs {
    fn into_request(self) -> ingest::Request {
        let change = self
            .remove
            .map(|id| (ingest::Change::Remove, id))
            .or(self.first.map(|id| (ingest::Change::First, id)))
            .or(self.retry.map(|id| (ingest::Change::Retry, id)));
        ingest::Request {
            paths: self.paths,
            options: ingest::Options {
                intake: carrel_core::ingest::IntakeOptions {
                    each: self.each,
                    whole: self.whole,
                },
                dry_run: self.dry_run,
                wait: self.wait,
            },
            status: self.status,
            change,
        }
    }
}

#[derive(Debug, Subcommand)]
enum SettingsAction {
    /// List the settings and their values.
    List,
    /// Change one: `picker` or `editor` for this device (empty to clear), or a shared one
    /// such as `ingest.originals`.
    Set {
        /// The setting.
        key: String,
        /// Its new value.
        value: String,
    },
}

#[derive(Debug, Subcommand)]
enum CollectionAction {
    /// Create a collection, such as `Finances` or `Finances/Tax`, with a design document to
    /// fill in. Models read it before filing anything there, so say what belongs and what
    /// doesn't.
    New {
        /// The collection's path: folder names separated by `/`. A parent must already be a
        /// collection.
        path: String,
        /// One line saying what belongs in it.
        #[arg(long)]
        description: Option<String>,
        /// A tag for its items and what it means, as `name=meaning`; repeat for more.
        #[arg(long = "tag", value_name = "NAME=MEANING")]
        tags: Vec<String>,
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
        /// File them as new versions of an archived item, with a `version-of` link. Give the
        /// item's identifier, or leave it out when each is a duplicate of just one item.
        #[arg(long, value_name = "ITEM", num_args = 0..=1, default_missing_value = "",
              conflicts_with_all = ["all_high", "related_to"])]
        as_version_of: Option<String>,
        /// File them beside an archived item, with a `related` link. Give the item's
        /// identifier, or leave it out when each is a duplicate of just one item.
        #[arg(long, value_name = "ITEM", num_args = 0..=1, default_missing_value = "",
              conflicts_with = "all_high")]
        related_to: Option<String>,
    },
    /// File proposals' items in Holding, to be placed later.
    Hold {
        /// The proposals' identifiers, or their starts or ends.
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Edit a proposal's card in your editor (the `editor` setting, $VISUAL or $EDITOR)
    /// before approving it.
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

/// `carrel review` and its subcommands.
fn review_command(
    archive: Option<&Path>,
    all: bool,
    action: Option<ReviewAction>,
) -> Result<Report, Failure> {
    match action.unwrap_or(ReviewAction::List { all }) {
        ReviewAction::List { all } => review::list(archive, all),
        ReviewAction::Show { id } => review::show(archive, &id),
        ReviewAction::Reject { ids } => review::reject(archive, &ids),
        ReviewAction::Approve {
            ids,
            all_high,
            as_version_of,
            related_to,
        } => review::approve(
            archive,
            &ids,
            all_high,
            as_version_of.as_deref(),
            related_to.as_deref(),
        ),
        ReviewAction::Hold { ids } => review::hold(archive, &ids),
        ReviewAction::Edit { id } => review::edit(archive, &id),
        ReviewAction::MoveTo { id, collection } => review::move_to(archive, &id, &collection),
    }
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
        Command::Ingest(args) => {
            ingest::command(cli.archive.as_deref(), &args.into_request(), cli.json)
        }
        Command::Worker => worker::run(cli.archive.as_deref()),
        Command::Settings { action } => match action {
            Some(SettingsAction::Set { key, value }) => {
                settings::set(cli.archive.as_deref(), &key, &value)
            }
            _ => settings::list(cli.archive.as_deref()),
        },
        Command::Review { all, action } => review_command(cli.archive.as_deref(), all, action),
        Command::Rename { item, new_name } => {
            commands::rename(cli.archive.as_deref(), &item, new_name.as_deref())
        }
        Command::Checkout { item, to, due } => {
            checkout::checkout(cli.archive.as_deref(), &item, to.as_deref(), due.as_deref())
        }
        Command::Checkouts => checkout::list(cli.archive.as_deref()),
        Command::Return {
            item,
            from,
            yes,
            discard,
        } => checkout::return_item(cli.archive.as_deref(), &item, from.as_deref(), yes, discard),
        Command::Collection { action } => match action {
            CollectionAction::New {
                path,
                description,
                tags,
            } => commands::collection_new(cli.archive.as_deref(), &path, description, &tags),
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
        Command::Bench(args) => bench::bench(&args, cli.json),
        Command::Models { action } => match action.unwrap_or(ModelsAction::List) {
            ModelsAction::List => models::list(),
            ModelsAction::Download { name, format, yes } => {
                models::download(&name, format, yes, cli.json)
            }
            ModelsAction::Remove { name, format, yes } => models::remove(&name, format, yes),
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
