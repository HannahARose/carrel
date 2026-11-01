//! `carrel discover`: the discovery views (`docs/design/librarian.md`, Discovery), computed by
//! `carrel_core::discover` and drawn here as tables and trees, or `--json`.

use std::fmt::Write as _;
use std::io::{BufRead as _, IsTerminal as _, Write as _};
use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::card::Sensitivity;
use carrel_core::discover::activity::{self, Activity};
use carrel_core::discover::map::{Map, Region};
use carrel_core::discover::timeline::{self, Options, Resolution, Timeline};
use carrel_core::discover::today::{self, OnThisDay};
use carrel_core::discover::visits::{Forgotten, Visits};
use carrel_core::discover::wander::{self, Reason, Step, Surroundings, Walk};
use carrel_core::discover::{Catalog, DateKind, Item, map};
use carrel_core::graph::{Connection, Edge, Graph};
use carrel_core::human_size;
use carrel_core::search::query::Query;
use jiff::civil::Date;
use serde_json::{Value, json};

use crate::commands::{find_item, locate, plural};
use crate::{Failure, Report};

/// Today, on this device's clock.
pub fn today() -> Date {
    jiff::Zoned::now().date()
}

fn parse_date(text: &str, what: &str) -> Result<Date, Failure> {
    text.parse().map_err(|_| {
        Failure::new(format!(
            "{what} needs a date such as 2025-03-14, not “{text}”"
        ))
    })
}

/// How an item is marked when it's sensitive or restricted, as search marks results.
fn mark(item: &Item) -> &'static str {
    crate::related::mark(item.sensitivity)
}

/// The device's record of discovery for `archive`, and where it's kept.
fn visits(archive: &Archive) -> (Visits, Option<std::path::PathBuf>) {
    let path = Visits::path(&archive.info().id.to_string());
    let visits = path.as_deref().map(Visits::load).unwrap_or_default();
    (visits, path)
}

fn save(visits: &Visits, path: Option<&Path>) {
    // Discovery's record is a convenience: a device that can't write it still discovers.
    if let Some(path) = path {
        let _ = visits.save(path);
    }
}

/// Records on this device that the collection `collection` was opened, as `carrel show` does.
pub fn opened(archive: &Archive, collection: &str) {
    let (mut visits, path) = visits(archive);
    if visits.open(collection, today()) {
        save(&visits, path.as_deref());
    }
}

/// `carrel discover today`.
pub fn on_this_day(archive: Option<&Path>, date: Option<&str>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let date = date.map_or_else(|| Ok(today()), |d| parse_date(d, "--date"))?;
    let day = today::on_this_day(&Catalog::read(&archive)?, date);
    Ok(Report::new(
        today_text(&day, date),
        serde_json::to_value(&day)?,
    ))
}

fn what_text(what: &[DateKind]) -> String {
    let words: Vec<&str> = what
        .iter()
        .map(|k| match k {
            DateKind::Created => "made",
            DateKind::Filed => "filed",
            DateKind::Worked => "worked on",
        })
        .collect();
    words.join(", ")
}

pub(crate) fn today_text(day: &OnThisDay, date: Date) -> String {
    let mut text = String::new();
    let heading = date.strftime("%-d %B").to_string();
    if day.years.is_empty() {
        writeln!(
            text,
            "Nothing was made, filed or worked on on {heading} in other years."
        )
        .unwrap();
        return text;
    }
    writeln!(text, "On this day, {heading}:").unwrap();
    for year in &day.years {
        let ago = match year.years_ago {
            1 => "a year ago".to_owned(),
            -1 => "a year later".to_owned(),
            n if n > 1 => format!("{n} years ago"),
            n => format!("{} years later", -n),
        };
        writeln!(text, "\n{} ({ago})", year.year).unwrap();
        for happened in &year.items {
            writeln!(
                text,
                "  {:<10} {}  {}{}",
                what_text(&happened.what),
                happened.item.title,
                happened.item.card,
                mark(&happened.item)
            )
            .unwrap();
        }
    }
    text
}

/// What `carrel discover wander` was asked for.
#[derive(Debug, Clone, Default)]
pub struct WanderArgs {
    /// The item to start from.
    pub item: Option<String>,
    /// The seed, for a walk that can be repeated.
    pub seed: Option<u64>,
    /// How many steps to take when not stepping one at a time.
    pub steps: usize,
    /// Go on with the last walk.
    pub next: bool,
}

fn new_seed() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    // Short enough to type back in.
    (now.as_secs() ^ u64::from(now.subsec_nanos())) % 1_000_000
}

fn start_walk(archive: &Archive, args: &WanderArgs, visits: &Visits) -> Result<Walk, Failure> {
    if args.next {
        return visits.walk.clone().ok_or_else(|| {
            Failure::new("there's no walk to go on with; `carrel discover wander` starts one")
        });
    }
    let start = match &args.item {
        Some(query) => {
            let items = archive.items()?;
            Some(find_item(archive, &items, query)?.id)
        }
        None => None,
    };
    Ok(Walk::new(args.seed.unwrap_or_else(new_seed), start).remembering(visits))
}

/// `carrel discover wander`: one step at a time in a terminal, else `--steps` of them.
pub fn wander(archive: Option<&Path>, args: &WanderArgs, json: bool) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let catalog = Catalog::read(&archive)?;
    let (mut visits, path) = visits(&archive);
    let mut walk = start_walk(&archive, args, &visits)?;
    let index = carrel_core::index::related::open(&archive).ok();
    let near = index.as_ref().map(wander::near_in);
    let texts = carrel_core::search::texts::TextCache::of(&archive);
    let interactive = !json && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let mut steps = Vec::new();
    let mut questions: Vec<(usize, carrel_core::questions::QuestionEntry)> = Vec::new();
    let mut asked_at = None;
    let mut done = false;
    let mut around = Surroundings::new().with_texts(&texts);
    if let Some(near) = &near {
        around = around.with_near(near);
    }
    loop {
        let Some(step) = walk.step(&catalog, &around) else {
            done = true;
            break;
        };
        visits.open(&step.item.collection, today());
        let question = carrel_core::questions::moments::wander_asks(step.number, asked_at)
            .then(|| crate::asking::question_on(&archive, &step.item.card))
            .flatten();
        if let Some(question) = question {
            asked_at = Some(step.number);
            questions.push((step.number, question));
        }
        if interactive {
            print!("{}", step_text(&step, &catalog));
            if let Some((n, question)) = questions.last()
                && *n == step.number
            {
                print!("{}", crate::asking::wander_question_text(question));
            }
            steps.push(step);
            eprint!("\nEnter for the next step, q to stop: ");
            let _ = std::io::stderr().flush();
            let mut line = String::new();
            let read = std::io::stdin().lock().read_line(&mut line)?;
            if read == 0 || line.trim().eq_ignore_ascii_case("q") {
                break;
            }
            println!();
            continue;
        }
        steps.push(step);
        if steps.len() >= args.steps.max(1) {
            break;
        }
    }
    let seed = walk.seed;
    let visited = walk.visited.len();
    visits.walk = Some(walk);
    save(&visits, path.as_deref());
    let mut text = String::new();
    if !interactive {
        for (i, step) in steps.iter().enumerate() {
            if i > 0 {
                text.push('\n');
            }
            text.push_str(&step_text(step, &catalog));
            if let Some((_, question)) = questions.iter().find(|(n, _)| *n == step.number) {
                text.push_str(&crate::asking::wander_question_text(question));
            }
        }
    }
    if done {
        writeln!(
            text,
            "\nThe walk has visited every item it can reach ({}).",
            plural(visited, "item")
        )
        .unwrap();
    } else {
        writeln!(
            text,
            "\nWalk {seed}: `carrel discover wander --next` takes the next step."
        )
        .unwrap();
    }
    let json = json!({
        "seed": seed,
        "steps": steps,
        "visited": visited,
        "done": done,
        "questions": questions.iter().map(|(n, q)| json!({
            "step": n,
            "id": q.id,
            "question": q.question,
            "subject": q.subject,
        })).collect::<Vec<_>>(),
    });
    Ok(Report::new(text, json))
}

fn reason_text(reason: &Reason, catalog: &Catalog) -> String {
    let title = |id| {
        catalog
            .get(id)
            .map_or_else(|| id.to_string(), |e| format!("“{}”", e.item.title))
    };
    match reason {
        Reason::Start { collection } => format!("A place to start: {collection}."),
        Reason::Linked {
            link_type,
            from,
            back,
        } => {
            if *back {
                format!("It links to {} ({link_type}).", title(*from))
            } else {
                format!("{} links to it ({link_type}).", title(*from))
            }
        }
        Reason::Near { from, score } => {
            format!("Near {} in meaning ({score:.2}).", title(*from))
        }
        Reason::Neighbour { collection } => format!("Beside it in {collection}."),
        Reason::Jump {
            collection,
            last_seen,
        } => match last_seen {
            Some(day) => format!("A jump to {collection}, not seen since {day}."),
            None => format!("A jump to {collection}, never opened here."),
        },
    }
}

pub(crate) fn step_text(step: &Step, catalog: &Catalog) -> String {
    let mut text = String::new();
    writeln!(
        text,
        "{}. {}{}",
        step.number,
        step.item.title,
        mark(&step.item)
    )
    .unwrap();
    writeln!(text, "   {}", reason_text(&step.reason, catalog)).unwrap();
    writeln!(text, "   {}", step.item.card).unwrap();
    if let Some(summary) = &step.summary {
        writeln!(text, "   {summary}").unwrap();
    }
    if !step.tags.is_empty() {
        writeln!(text, "   tags: {}", step.tags.join(", ")).unwrap();
    }
    if let Some(preview) = &step.preview {
        writeln!(text, "   │ {preview}").unwrap();
    } else if step.item.sensitivity == Sensitivity::Restricted {
        writeln!(text, "   Restricted: its text isn't shown.").unwrap();
    }
    text
}

/// What `carrel discover timeline` was asked for.
#[derive(Debug, Clone, Default)]
pub struct TimelineArgs {
    /// Words and search's filters.
    pub query: Vec<String>,
    /// Which date: created, filed or worked.
    pub by: Option<String>,
    /// Year, month or day.
    pub per: String,
    /// How many items to list in each period.
    pub items: usize,
}

/// `carrel discover timeline`.
pub fn timeline(archive: Option<&Path>, args: &TimelineArgs) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let query = Query::parse(&args.query.join(" "))?;
    let by = match &args.by {
        Some(by) => Some(DateKind::parse(by).ok_or_else(|| {
            Failure::new(format!("--by is created, filed or worked, not “{by}”"))
        })?),
        None => None,
    };
    let resolution = Resolution::parse(&args.per)
        .ok_or_else(|| Failure::new(format!("--per is year, month or day, not “{}”", args.per)))?;
    let options = Options {
        by,
        resolution,
        filters: query.filters.clone(),
        only: timeline::matching(&archive, &query)?,
    };
    let found = timeline::timeline(&Catalog::read(&archive)?, &options);
    Ok(Report::new(
        timeline_text(&found, args.items),
        serde_json::to_value(&found)?,
    ))
}

pub(crate) fn timeline_text(timeline: &Timeline, listed: usize) -> String {
    let mut text = String::new();
    if timeline.periods.is_empty() {
        writeln!(text, "Nothing is on the timeline.").unwrap();
    }
    let widest = timeline
        .periods
        .iter()
        .map(|p| p.items.len())
        .max()
        .unwrap_or(0);
    for period in &timeline.periods {
        let bar = "█".repeat(bar_width(period.items.len(), widest, 30));
        writeln!(
            text,
            "{:<10} {:>5}  {bar}",
            period.period,
            period.items.len()
        )
        .unwrap();
        for placed in period.items.iter().take(listed) {
            writeln!(
                text,
                "             {}  {}  {}{}",
                placed.date,
                placed.item.title,
                placed.item.card,
                mark(&placed.item)
            )
            .unwrap();
        }
        if period.items.len() > listed && listed > 0 {
            writeln!(
                text,
                "             … and {} more",
                period.items.len() - listed
            )
            .unwrap();
        }
    }
    if timeline.undated > 0 {
        let what = match timeline.by {
            Some(DateKind::Created) => "no date made",
            Some(DateKind::Filed) => "no date filed",
            Some(DateKind::Worked) => "never been worked on",
            None => "no date",
        };
        writeln!(text, "\n{} {what}.", plural_have(timeline.undated)).unwrap();
    }
    text
}

fn plural_have(n: usize) -> String {
    if n == 1 {
        "1 item has".to_owned()
    } else {
        format!("{n} items have")
    }
}

/// How many of `width` cells `n` of `most` fills, at least one for anything.
fn bar_width(n: usize, most: usize, width: usize) -> usize {
    if n == 0 || most == 0 {
        0
    } else {
        (n * width).div_ceil(most).clamp(1, width)
    }
}

/// `carrel discover map`.
pub fn map(archive: Option<&Path>, depth: Option<usize>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let found = map::map(&Catalog::read(&archive)?);
    Ok(Report::new(
        map_text(&found, depth),
        serde_json::to_value(&found)?,
    ))
}

pub(crate) fn map_text(map: &Map, depth: Option<usize>) -> String {
    let mut text = String::new();
    writeln!(
        text,
        "{} in {}",
        plural(map.total_items, "item"),
        human_size(map.total_size)
    )
    .unwrap();
    for (i, region) in map.regions.iter().enumerate() {
        region_text(
            &mut text,
            region,
            map.total_size,
            "",
            i + 1 == map.regions.len(),
            depth,
            1,
        );
    }
    if map.without_size > 0 {
        writeln!(
            text,
            "\n{} no size on {}, so {} counted in the sizes.",
            plural_have(map.without_size),
            if map.without_size == 1 {
                "its card"
            } else {
                "their cards"
            },
            if map.without_size == 1 {
                "it isn't"
            } else {
                "they aren't"
            }
        )
        .unwrap();
    }
    text
}

fn region_text(
    text: &mut String,
    region: &Region,
    total: u64,
    indent: &str,
    last: bool,
    depth: Option<usize>,
    level: usize,
) {
    let branch = if last { "└─ " } else { "├─ " };
    let share = region
        .total_size
        .saturating_mul(100)
        .checked_div(total)
        .map_or(0, |s| usize::try_from(s).unwrap_or(100));
    let bar = "█".repeat(bar_width(share, 100, 20));
    let name = region.path.rsplit('/').next().unwrap_or(&region.path);
    let label = format!("{indent}{branch}{name}");
    writeln!(
        text,
        "{label:<36} {:>9} {:>10}  {bar}",
        human_size(region.total_size),
        plural(region.total_items, "item"),
    )
    .unwrap();
    if depth.is_some_and(|d| level >= d) {
        return;
    }
    let inner = format!("{indent}{}", if last { "   " } else { "│  " });
    for (i, child) in region.children.iter().enumerate() {
        region_text(
            text,
            child,
            total,
            &inner,
            i + 1 == region.children.len(),
            depth,
            level + 1,
        );
    }
}

/// `carrel discover activity`.
pub fn activity(
    archive: Option<&Path>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let (default_from, default_to) = activity::last_year(today());
    let to = to.map_or(Ok(default_to), |d| parse_date(d, "--to"))?;
    let from = from.map_or_else(
        || {
            Ok(if to == default_to {
                default_from
            } else {
                activity::last_year(to).0
            })
        },
        |d| parse_date(d, "--from"),
    )?;
    if from > to {
        return Err(Failure::new("--from must come before --to"));
    }
    let found = activity::activity(&Catalog::read(&archive)?, from, to);
    Ok(Report::new(
        activity_text(&found),
        serde_json::to_value(&found)?,
    ))
}

/// `carrel discover graph`: the strongest connections between collections, or with `items`
/// between items in different collections, at most `limit` of each. `--json` gives both.
pub fn graph(archive: Option<&Path>, items: bool, limit: usize) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let found = Graph::build(&Catalog::read(&archive)?);
    let connections: Vec<Connection> = found.connections().into_iter().take(limit).collect();
    let edges: Vec<&Edge> = found
        .strongest_item_edges()
        .into_iter()
        .take(limit)
        .collect();
    let text = graph_text(&found, items, limit);
    let node = |id| {
        found.item(id).map(|i| {
            json!({
                "id": i.id,
                "title": i.title,
                "card": i.card,
                "collection": i.collection,
                "sensitivity": i.sensitivity,
            })
        })
    };
    let json = json!({
        "connections": connections,
        "items": edges.iter().map(|e| json!({
            "a": node(e.a),
            "b": node(e.b),
            "weight": e.weight,
            "tags": e.tags,
            "links": e.links,
            "days": e.days,
        })).collect::<Vec<_>>(),
    });
    Ok(Report::new(text, json))
}

/// The graph's `limit` strongest connections, as `carrel discover graph` writes them:
/// between collections, or with `items` between items in different collections.
pub(crate) fn graph_text(found: &Graph, items: bool, limit: usize) -> String {
    if items {
        let edges: Vec<&Edge> = found
            .strongest_item_edges()
            .into_iter()
            .take(limit)
            .collect();
        item_edges_text(found, &edges)
    } else {
        let connections: Vec<Connection> = found.connections().into_iter().take(limit).collect();
        connections_text(&connections)
    }
}

/// Why two things are connected, in words: their links, days worked on together and tags.
fn ties_text(links: usize, link_types: &[String], days: usize, tags: &[String]) -> String {
    let mut parts = Vec::new();
    if links > 0 && link_types.is_empty() {
        parts.push(plural(links, "link"));
    } else if links > 0 {
        let mut types = link_types.to_vec();
        types.dedup();
        parts.push(format!("{} ({})", plural(links, "link"), types.join(", ")));
    }
    if days > 0 {
        parts.push(format!("worked on together on {}", plural(days, "day")));
    }
    if !tags.is_empty() {
        let shown: Vec<&str> = tags.iter().take(5).map(String::as_str).collect();
        let more = tags.len().saturating_sub(shown.len());
        let mut listed = shown.join(", ");
        if more > 0 {
            write!(listed, " and {more} more").unwrap();
        }
        parts.push(format!("tags {listed}"));
    }
    parts.join("; ")
}

pub(crate) fn connections_text(connections: &[Connection]) -> String {
    if connections.is_empty() {
        return "No collections are connected yet: their items share no tags, links or days \
                of work.\n"
            .into();
    }
    let mut text = String::from("The strongest connections between collections:\n");
    for c in connections {
        let branches = if c.separate_branches {
            ""
        } else {
            " (one inside the other)"
        };
        writeln!(
            text,
            "  {} ↔ {}{branches}  {:.1}\n    {} tied: {}",
            c.a,
            c.b,
            c.weight,
            plural(c.pairs, "pair"),
            ties_text(
                c.links,
                &[],
                c.days,
                &c.tags
                    .iter()
                    .map(|t| format!("{} ({})", t.tag, t.pairs))
                    .collect::<Vec<_>>()
            )
        )
        .unwrap();
    }
    text
}

pub(crate) fn item_edges_text(graph: &Graph, edges: &[&Edge]) -> String {
    if edges.is_empty() {
        return "No items in different collections are connected yet.\n".into();
    }
    let name = |id| {
        graph.item(id).map_or_else(String::new, |i| {
            format!(
                "{}{} ({})",
                i.title,
                crate::related::mark(i.sensitivity),
                i.collection
            )
        })
    };
    let mut text =
        String::from("The strongest connections between items in different collections:\n");
    for e in edges {
        let ties = ties_text(e.links.len(), &e.links, e.days, &e.tags);
        writeln!(
            text,
            "  {} ↔ {}  {:.1}\n    {ties}",
            name(e.a),
            name(e.b),
            e.weight
        )
        .unwrap();
    }
    text
}

/// The shades of a heatmap's cells, from nothing to the busiest.
const SHADES: [char; 5] = ['·', '░', '▒', '▓', '█'];

pub(crate) fn activity_text(activity: &Activity) -> String {
    let mut text = String::new();
    let filed: usize = activity.days.iter().map(|d| d.filed).sum();
    let sessions: usize = activity.days.iter().map(|d| d.sessions).sum();
    writeln!(
        text,
        "{} filed and {} from {} to {}",
        plural(filed, "item"),
        plural(sessions, "work session"),
        activity.from,
        activity.to
    )
    .unwrap();
    // Weeks across, Monday to Sunday down, as a heatmap is drawn.
    let back = i64::from(activity.from.weekday().to_monday_zero_offset());
    let first = activity
        .from
        .checked_sub(jiff::Span::new().days(back))
        .unwrap_or(activity.from);
    let mut weeks: Vec<Date> = Vec::new();
    let mut monday = first;
    while monday <= activity.to {
        weeks.push(monday);
        monday = monday
            .checked_add(jiff::Span::new().days(7))
            .unwrap_or(Date::MAX);
    }
    let mut header = vec![' '; 4 + weeks.len() + 3];
    let mut free = 0;
    for (i, week) in weeks.iter().enumerate() {
        let column = 4 + i;
        // The first week is named only when its month has room before the next begins.
        let starts = if i == 0 {
            weeks
                .get(1..4)
                .is_none_or(|next| next.iter().all(|w| w.month() == week.month()))
        } else {
            week.month() != weeks[i - 1].month()
        };
        if starts && column >= free {
            for (j, c) in week.strftime("%b").to_string().chars().enumerate() {
                header[column + j] = c;
            }
            free = column + 4;
        }
    }
    let months: String = header.into_iter().collect();
    writeln!(text, "\n{}", months.trim_end()).unwrap();
    for (row, name) in ["Mon", "", "Wed", "", "Fri", "", "Sun"].iter().enumerate() {
        write!(text, "{name:<4}").unwrap();
        for week in &weeks {
            let day = week
                .checked_add(jiff::Span::new().days(i64::try_from(row).unwrap_or(0)))
                .unwrap_or(*week);
            if day < activity.from || day > activity.to {
                text.push(' ');
            } else {
                text.push(SHADES[activity.level(&activity.on(day), SHADES.len() - 1)]);
            }
        }
        text.push('\n');
    }
    writeln!(
        text,
        "\nLess {} More   (busiest day: {})",
        SHADES.iter().collect::<String>(),
        plural(activity.busiest, "thing")
    )
    .unwrap();
    text
}

/// The part of `carrel status` that now and then names a collection not opened in a long
/// time, and the same for programs.
pub fn resurfaced(archive: &Archive, records: &mut crate::asking::Records) -> (String, Value) {
    let Ok(catalog) = Catalog::read(archive) else {
        return (String::new(), Value::Null);
    };
    let before = records.visits.resurfaced.clone();
    let found = records.visits.resurface(&catalog, today());
    if records.visits.resurfaced != before {
        records.save_visits();
    }
    let Some(found) = found else {
        return (String::new(), Value::Null);
    };
    (
        resurfaced_text(&found),
        serde_json::to_value(&found).unwrap_or(Value::Null),
    )
}

fn resurfaced_text(found: &Forgotten) -> String {
    let mut text = String::new();
    writeln!(
        text,
        "\nNot opened in a while: {} ({}), last seen {}.",
        found.title, found.collection, found.last_seen
    )
    .unwrap();
    for item in &found.items {
        writeln!(text, "  {}  {}{}", item.title, item.card, mark(item)).unwrap();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_fill_in_proportion() {
        assert_eq!(bar_width(0, 10, 30), 0);
        assert_eq!(bar_width(1, 1000, 30), 1, "anything shows");
        assert_eq!(bar_width(10, 10, 30), 30);
    }
}
