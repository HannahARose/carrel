//! `carrel report gaps`: what Carrel couldn't do on this device (`carrel_core::gaps`), and
//! reporting a gap to development as a GitHub issue draft the owner opens in their browser,
//! edits and submits themself. Carrel never sends it. The interactive mode shows the same
//! list in its Gaps view.

use std::fmt::Write as _;
use std::path::Path;

use carrel_core::gaps::draft::IssueDraft;
use carrel_core::gaps::{Gap, GapLog};
use serde_json::json;

use crate::commands::{find_item, locate};
use crate::{Failure, Report};

/// `carrel report`'s options: the usage report's, or what else it does.
#[derive(Debug, clap::Args)]
pub struct ReportArgs {
    #[command(subcommand)]
    action: Option<ReportAction>,
    /// The period: a span such as 14d or 2w, `today`, or a date such as 2026-10-01.
    #[arg(long, value_name = "WHEN", default_value = crate::usage_report::DEFAULT_SINCE)]
    since: String,
    /// Show collections by path rather than identifier, for your eyes only.
    #[arg(long)]
    names: bool,
}

/// What `carrel report` does besides the usage report.
#[derive(Debug, clap::Subcommand)]
pub enum ReportAction {
    /// What Carrel couldn't do on this device, such as file types it can't read, with no
    /// titles, text, paths or file names, and a way to report each to development.
    Gaps(GapsArgs),
}

/// `carrel report gaps`'s options. With none, it lists the gaps.
#[derive(Debug, clap::Args)]
pub struct GapsArgs {
    /// Draft an issue for gap N on GitHub, and open it in the browser to read, edit and
    /// submit. Nothing is sent until you submit it there.
    #[arg(long, value_name = "N", conflicts_with_all = ["reported", "missing"])]
    pub report: Option<u32>,
    /// With --report, print the draft's address rather than opening it.
    #[arg(long, requires = "report")]
    pub print: bool,
    /// With --report, words of your own to add to the draft.
    #[arg(long, value_name = "TEXT", requires = "report")]
    pub add: Option<String>,
    /// Record that gap N was reported in the issue at URL (an empty URL clears it).
    #[arg(long, num_args = 2, value_names = ["N", "URL"], conflicts_with = "missing")]
    pub reported: Option<Vec<String>>,
    /// Report a missing feature yourself, in your own words.
    #[arg(long, value_name = "WORDS")]
    pub missing: Option<String>,
    /// With --missing, the item it's about: its type is recorded, never its name.
    #[arg(long, value_name = "ITEM", requires = "missing")]
    pub item: Option<String>,
}

/// `carrel report`: what `args` ask, or the usage report.
///
/// # Errors
///
/// [`Failure`] as each action says.
pub fn command(archive: Option<&Path>, args: ReportArgs) -> Result<Report, Failure> {
    match args.action {
        None => crate::usage_report::report(archive, &args.since, args.names),
        Some(ReportAction::Gaps(gaps_args)) => gaps(archive, gaps_args),
    }
}

/// `carrel report gaps`.
fn gaps(archive: Option<&Path>, args: GapsArgs) -> Result<Report, Failure> {
    let config = carrel_core::local::config_folder()?;
    if let Some(number) = args.report {
        return report_gap(&config, number, args.add.as_deref(), args.print);
    }
    if let Some(reported) = args.reported {
        let number = gap_number(&reported[0])?;
        let gap = carrel_core::gaps::mark_reported_in(&config, number, &reported[1])
            .map_err(|e| Failure::new(e.to_string()))?;
        let text = match &gap.reported {
            Some(address) => format!("Gap {number} was reported at {address}\n"),
            None => format!("Gap {number} is no longer marked as reported\n"),
        };
        return Ok(Report::new(text, json!({ "gap": gap })));
    }
    if let Some(words) = args.missing {
        let archive = args.item.as_ref().map(|_| locate(archive)).transpose()?;
        let items = match &archive {
            Some(archive) => archive.items()?,
            None => Vec::new(),
        };
        let item = match (&archive, &args.item) {
            (Some(archive), Some(query)) => Some(find_item(archive, &items, query)?),
            _ => None,
        };
        let gap =
            carrel_core::gaps::report_missing_feature_in(&config, archive.as_ref(), item, &words)
                .map_err(|e| Failure::new(e.to_string()))?;
        let text = format!(
            "Recorded gap {}: {}\n`carrel report gaps --report {}` drafts an issue for development.\n",
            gap.number,
            gap.summary(),
            gap.number
        );
        return Ok(Report::new(text, json!({ "gap": gap })));
    }
    let log = carrel_core::gaps::list_in(&config);
    let mut text = gap_lines(&log).join("\n");
    text.push('\n');
    if !log.gaps.is_empty() {
        text.push_str(
            "\nNo titles, text, paths or file names are in it. `carrel report gaps --report N` drafts an issue for development in your browser; nothing is sent until you submit it there.\n",
        );
    }
    Ok(Report::new(
        text,
        json!({ "version": carrel_core::CARREL_VERSION, "gaps": log.gaps }),
    ))
}

/// `--report N`: the draft for gap `number`, opened in the browser unless `print`.
fn report_gap(
    config: &Path,
    number: u32,
    added: Option<&str>,
    print: bool,
) -> Result<Report, Failure> {
    let log = carrel_core::gaps::list_in(config);
    let gap = log
        .get(number)
        .ok_or_else(|| Failure::new(format!("there's no gap {number}")))?;
    let draft = issue_draft(gap, added);
    let url = draft.url();
    let text = if print {
        format!("{url}\n")
    } else {
        carrel_core::gaps::draft::open_in_browser(&url).map_err(|e| {
            Failure::new(format!(
                "couldn't open the browser ({e}); `--print` gives the address to open yourself"
            ))
        })?;
        format!(
            "Opened an issue draft for gap {number} in your browser: read and edit it, then submit it there.\nOnce it's submitted, `carrel report gaps --reported {number} <its address>` records it.\n"
        )
    };
    Ok(Report::new(
        text,
        json!({
            "gap": number,
            "title": draft.title,
            "body": draft.body,
            "url": url,
            "opened": !print,
        }),
    ))
}

/// The issue draft for `gap` from this Carrel, with the owner's `added` words.
#[must_use]
pub fn issue_draft(gap: &Gap, added: Option<&str>) -> IssueDraft {
    IssueDraft::of(
        gap,
        carrel_core::CARREL_VERSION,
        std::env::consts::OS,
        added,
    )
}

fn gap_number(text: &str) -> Result<u32, Failure> {
    text.trim()
        .parse()
        .map_err(|_| Failure::new(format!("a gap is named by its number, not “{text}”")))
}

/// The gaps in `log`, as `carrel report gaps` and the interactive mode list them.
#[must_use]
pub fn gap_lines(log: &GapLog) -> Vec<String> {
    if log.gaps.is_empty() {
        return vec!["No gaps: Carrel has done all it was asked on this device so far.".into()];
    }
    let mut lines = vec!["What Carrel couldn't do on this device, most items first:".to_owned()];
    for gap in &log.gaps {
        lines.push(format!("{:>4}  {}", gap.number, gap.summary()));
        let mut detail = format!(
            "      {}, {} to {}, Carrel {}",
            crate::commands::plural(usize::try_from(gap.items).unwrap_or(usize::MAX), "item"),
            gap.first_seen,
            gap.last_seen,
            gap.last_version
        );
        if let Some(words) = &gap.words {
            write!(detail, "\n      “{words}”").unwrap();
        }
        match &gap.reported {
            Some(address) => {
                write!(detail, "\n      Reported: {address}").unwrap();
                if gap.not_seen_since_update(carrel_core::CARREL_VERSION) {
                    write!(
                        detail,
                        " (not seen since Carrel {}, so this version may have addressed it)",
                        gap.last_version
                    )
                    .unwrap();
                }
            }
            None if gap.not_seen_since_update(carrel_core::CARREL_VERSION) => {
                write!(detail, "\n      Not seen since Carrel {}", gap.last_version).unwrap();
            }
            None => {}
        }
        lines.extend(detail.lines().map(ToOwned::to_owned));
    }
    lines
}

#[cfg(test)]
mod tests {
    use carrel_core::gaps::{GapKind, Sighting};

    use super::*;

    #[test]
    fn gaps_are_listed_with_their_counts_and_reports() {
        assert!(gap_lines(&GapLog::default())[0].starts_with("No gaps"));
        let mut log = GapLog::default();
        let n = log.note(
            &Sighting::new(GapKind::UnreadFormat, "model/3mf", "3mf", "text extraction"),
            3,
            "2026-10-30",
            carrel_core::CARREL_VERSION,
        );
        log.mark_reported(n, "https://github.com/Horizon-Labs/carrel/issues/1")
            .unwrap();
        let lines = gap_lines(&log).join("\n");
        assert!(
            lines.contains("   1  no reader for the file type: .3mf"),
            "{lines}"
        );
        assert!(
            lines.contains("3 items, 2026-10-30 to 2026-10-30"),
            "{lines}"
        );
        assert!(lines.contains("Reported: https://github.com/"), "{lines}");
    }
}
