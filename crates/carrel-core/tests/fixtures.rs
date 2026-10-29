//! The sample archive conforms to FORMAT.md, checked the way an independent reader would.
//!
//! [`check`] verifies what can be verified from the files alone:
//! - every card's frontmatter against `schemas/card.schema.json`, with a JSON Schema validator
//!   that knows nothing of the core's types
//! - identifiers, item checksums, sizes and manifests
//! - tag vocabularies and links
//! - navigation from `ARCHIVE.md`
//! - that nothing is left uncataloged
//!
//! The other tests break copies of the archive to show each check catches what it should.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Component, Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::card::{Checksum, Kind};
use carrel_core::card_file::CardFile;
use carrel_core::detect::is_ignored;
use carrel_core::frontmatter;
use carrel_core::manifest::Manifest;
use carrel_core::sections;
use serde_json::Value;

fn sample_archive() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Every file under `dir`, relative to `root`, skipping ignored names.
fn files_under(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap())
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_ignored(&name) {
            continue;
        }
        if entry.file_type().unwrap().is_dir() {
            files_under(root, &entry.path(), out);
        } else {
            out.push(relative(root, &entry.path()));
        }
    }
}

fn has_extension(path: &str, extension: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(extension))
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

/// `href` resolved against the folder `from` (archive-relative), with `.` and `..` removed.
fn resolve(from: &str, href: &str) -> String {
    let mut parts: Vec<String> = if from.is_empty() {
        Vec::new()
    } else {
        from.split('/').map(str::to_owned).collect()
    };
    for component in Path::new(href).components() {
        match component {
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            _ => {}
        }
    }
    parts.join("/")
}

fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Decodes `%XX` escapes in a Markdown link destination.
fn percent_decode(href: &str) -> String {
    let bytes = href.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = href.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

/// The destinations of the Markdown links in `text`, as written.
fn link_destinations(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("](") {
        rest = &rest[start + 2..];
        if let Some(end) = rest.find([')', '\n'])
            && rest.as_bytes()[end] == b')'
        {
            out.push(&rest[..end]);
            rest = &rest[end + 1..];
        }
    }
    out
}

fn is_external(href: &str) -> bool {
    href.starts_with('#')
        || href.split_once(':').is_some_and(|(scheme, _)| {
            !scheme.is_empty() && scheme.bytes().all(|b| b.is_ascii_lowercase())
        })
}

/// What a check found wrong, one line each.
#[derive(Default)]
struct Problems(Vec<String>);

impl Problems {
    fn add(&mut self, problem: impl Into<String>) {
        self.0.push(problem.into());
    }
}

/// Checks the archive at `root`, returning every problem found.
fn check(root: &Path) -> Vec<String> {
    let mut problems = Problems::default();
    let archive = match Archive::open(root) {
        Ok(archive) => archive,
        Err(e) => return vec![format!("can't open the archive: {e}")],
    };
    let collections = archive.collections().unwrap();
    let items = archive.items().unwrap();

    let cards = find_cards(root);
    for card in &cards {
        if !items.iter().any(|i| &i.card == card) {
            let text = std::fs::read_to_string(root.join(card)).unwrap();
            let why = CardFile::parse(&text)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            problems.add(format!("{card}: the core doesn't read it as a card: {why}"));
        }
    }
    check_identifiers(&archive, &collections, &items, &mut problems);
    check_schema(root, &cards, &mut problems);
    check_items(&archive, &items, &mut problems);
    check_tags_and_links(&archive, &collections, &items, &mut problems);
    check_cataloged(root, &collections, &items, &mut problems);
    check_markdown_links(root, &mut problems);
    check_navigation(root, &collections, &items, &mut problems);
    check_system_files(root, &mut problems);
    check_review_entries(&archive, &collections, &mut problems);
    problems.0
}

fn check_identifiers(
    archive: &Archive,
    collections: &[carrel_core::archive::Collection],
    items: &[carrel_core::archive::ItemRef],
    problems: &mut Problems,
) {
    let mut seen = BTreeMap::new();
    let mut claim = |id: String, owner: &str, problems: &mut Problems| {
        if let Some(first) = seen.insert(id.clone(), owner.to_owned()) {
            problems.add(format!("{owner}: identifier {id} is also used by {first}"));
        }
    };
    claim(archive.info().id.to_string(), "ARCHIVE.md", problems);
    for c in collections {
        claim(
            c.doc.id.to_string(),
            &format!("{}/_collection.md", c.path),
            problems,
        );
        if !archive.root().join(&c.path).join("CHANGELOG.md").is_file() {
            problems.add(format!("{}: no CHANGELOG.md", c.path));
        }
    }
    for item in items {
        claim(item.id.to_string(), &item.card, problems);
        if !item.id.is_v7() {
            problems.add(format!("{}: identifier isn't a UUIDv7", item.card));
        }
    }
}

/// Every Markdown file outside `_System` whose frontmatter says `carrel: card`, found without
/// the core's help.
fn find_cards(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    files_under(root, root, &mut files);
    files
        .into_iter()
        .filter(|f| has_extension(f, "md") && !f.starts_with("_System/"))
        .filter(|f| {
            let text = std::fs::read_to_string(root.join(f)).unwrap();
            frontmatter::split(&text)
                .ok()
                .and_then(|split| serde_saphyr::from_str::<Value>(split.yaml).ok())
                .is_some_and(|value| value["carrel"] == "card")
        })
        .collect()
}

fn check_schema(root: &Path, cards: &[String], problems: &mut Problems) {
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("schemas/card.schema.json")).unwrap(),
    )
    .unwrap();
    let validator = jsonschema::validator_for(&schema).expect("the card schema is valid");
    for card in cards {
        let text = std::fs::read_to_string(root.join(card)).unwrap();
        if text.contains("\r\n") {
            problems.add(format!("{card}: CRLF line endings"));
        }
        let split = match frontmatter::split(&text) {
            Ok(split) => split,
            Err(e) => {
                problems.add(format!("{card}: {e:?}"));
                continue;
            }
        };
        let value: Value = match serde_saphyr::from_str(split.yaml) {
            Ok(value) => value,
            Err(e) => {
                problems.add(format!("{card}: YAML: {e}"));
                continue;
            }
        };
        for error in validator.iter_errors(&value) {
            problems.add(format!(
                "{}: schema: {} at {}",
                card,
                error,
                error.instance_path()
            ));
        }
        match CardFile::parse(&text) {
            Ok(file) => {
                for problem in file.card().problems() {
                    problems.add(format!("{card}: {problem}"));
                }
                if file.to_text().unwrap() != text {
                    problems.add(format!("{card}: doesn't round-trip byte for byte"));
                }
            }
            Err(e) => problems.add(format!("{card}: {e}")),
        }
    }
}

fn check_items(
    archive: &Archive,
    items: &[carrel_core::archive::ItemRef],
    problems: &mut Problems,
) {
    let root = archive.root();
    let rules = archive.rules().unwrap();
    for item in items {
        let file =
            CardFile::parse(&std::fs::read_to_string(root.join(&item.card)).unwrap()).unwrap();
        let card = file.card();
        let Some(path) = &item.item else { continue };
        let full = root.join(path);
        if !full.exists() {
            problems.add(format!("{}: item {path} is missing", item.card));
            continue;
        }
        let name = path.rsplit('/').next().unwrap();
        let card_name = item.card.rsplit('/').next().unwrap();
        if card_name == format!("{name}.card.md") {
            let plain = format!("{path}.md");
            if !root.join(&plain).exists() || items.iter().any(|i| i.card == plain) {
                problems.add(format!(
                    "{}: uses .card.md although {plain} is free or is itself a card",
                    item.card
                ));
            }
        } else if card_name != format!("{name}.md") {
            problems.add(format!(
                "{}: card isn't named after its item (FORMAT.md §5.1)",
                item.card
            ));
        }
        if item.kind == Kind::File {
            if card.checksum.as_ref() != Some(&Checksum::of_file(&full).unwrap()) {
                problems.add(format!("{}: checksum doesn't match {path}", item.card));
            }
            if card.size != Some(std::fs::metadata(&full).unwrap().len()) {
                problems.add(format!("{}: size doesn't match {path}", item.card));
            }
            continue;
        }
        let Some(manifest_ref) = &card.manifest else {
            problems.add(format!("{}: no manifest", item.card));
            continue;
        };
        let Ok(text) = std::fs::read_to_string(root.join(&manifest_ref.path)) else {
            problems.add(format!(
                "{}: manifest {} is missing",
                item.card, manifest_ref.path
            ));
            continue;
        };
        let recorded = Manifest::parse(&text).unwrap();
        if recorded.checksum() != manifest_ref.checksum {
            problems.add(format!("{}: manifest checksum doesn't match", item.card));
        }
        let current = Manifest::build(&full, item.id, &rules, None).unwrap();
        let diff = recorded.diff(&current);
        if !diff.is_empty() {
            problems.add(format!("{}: manifest is out of date: {diff:?}", item.card));
        }
        if card.size != Some(current.total_size()) {
            problems.add(format!("{}: size doesn't match its contents", item.card));
        }
    }
}

fn check_tags_and_links(
    archive: &Archive,
    collections: &[carrel_core::archive::Collection],
    items: &[carrel_core::archive::ItemRef],
    problems: &mut Problems,
) {
    let root = archive.root();
    let custom_links: BTreeSet<&str> = archive
        .doc()
        .link_types
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    for item in items {
        let file =
            CardFile::parse(&std::fs::read_to_string(root.join(&item.card)).unwrap()).unwrap();
        let card = file.card();
        let folder = parent_of(&item.card);

        // The archive's tags, plus those of every collection containing the card.
        let mut vocabulary: BTreeSet<&str> = archive
            .doc()
            .tags
            .iter()
            .map(|t| t.name.0.as_str())
            .collect();
        for c in collections {
            if folder == c.path || folder.starts_with(&format!("{}/", c.path)) {
                vocabulary.extend(c.doc.tags.iter().map(|t| t.name.0.as_str()));
            }
        }
        for tag in &card.tags {
            if !vocabulary.contains(tag.0.as_str()) {
                problems.add(format!(
                    "{}: tag {} isn't in any vocabulary that applies",
                    item.card, tag.0
                ));
            }
        }

        for link in &card.links {
            let Some(target) = items.iter().find(|i| i.id == link.id) else {
                problems.add(format!("{}: link to unknown item {}", item.card, link.id));
                continue;
            };
            if let Some(path) = &link.path
                && resolve(folder, path) != target.card
            {
                problems.add(format!(
                    "{}: link path {path} doesn't lead to {}",
                    item.card, target.card
                ));
            }
            if !link.link_type.is_built_in() && !custom_links.contains(link.link_type.0.as_str()) {
                problems.add(format!(
                    "{}: unknown link type {}",
                    item.card, link.link_type.0
                ));
            }
        }
    }
}

fn check_cataloged(
    root: &Path,
    collections: &[carrel_core::archive::Collection],
    items: &[carrel_core::archive::ItemRef],
    problems: &mut Problems,
) {
    const RESERVED: [&str; 6] = [
        "ARCHIVE.md",
        "README.md",
        "Active",
        "Holding",
        "Journal",
        "_System",
    ];
    let collection_paths: BTreeSet<&str> = collections.iter().map(|c| c.path.as_str()).collect();
    let known: BTreeSet<&str> = items
        .iter()
        .flat_map(|i| std::iter::once(i.card.as_str()).chain(i.item.as_deref()))
        .collect();
    let names = |dir: &Path| -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| !is_ignored(n))
            .collect();
        names.sort();
        names
    };
    for name in names(root) {
        if !RESERVED.contains(&name.as_str()) && !collection_paths.contains(name.as_str()) {
            problems.add(format!("uncataloged at the root: {name}"));
        }
    }
    for c in collections {
        for name in names(&root.join(&c.path)) {
            let path = format!("{}/{name}", c.path);
            let structural = name == "_collection.md" || name == "CHANGELOG.md";
            if !structural
                && !collection_paths.contains(path.as_str())
                && !known.contains(path.as_str())
            {
                problems.add(format!("uncataloged: {path}"));
            }
        }
    }
}

fn check_markdown_links(root: &Path, problems: &mut Problems) {
    let mut files = Vec::new();
    files_under(root, root, &mut files);
    for file in files
        .iter()
        .filter(|f| has_extension(f, "md") && !f.starts_with("_System/"))
    {
        let text = std::fs::read_to_string(root.join(file)).unwrap();
        for href in link_destinations(&text) {
            if is_external(href) {
                continue;
            }
            if href.contains([' ', '(', ')', '<', '>']) {
                problems.add(format!(
                    "{file}: link destination isn't percent-encoded: {href}"
                ));
                continue;
            }
            let target = resolve(parent_of(file), &percent_decode(href));
            if !root.join(&target).exists() {
                problems.add(format!("{file}: broken link {href}"));
            }
        }
    }
}

fn check_navigation(
    root: &Path,
    collections: &[carrel_core::archive::Collection],
    items: &[carrel_core::archive::ItemRef],
    problems: &mut Problems,
) {
    let read = |path: &str| std::fs::read_to_string(root.join(path)).unwrap_or_default();
    let links_from = |path: &str, text: &str| -> Vec<String> {
        link_destinations(text)
            .into_iter()
            .filter(|h| !is_external(h))
            .map(|h| resolve(parent_of(path), &percent_decode(h)))
            .collect()
    };

    let archive_text = read("ARCHIVE.md");
    match sections::get(&archive_text, "collections") {
        Ok(Some(section)) => {
            let mut linked = links_from("ARCHIVE.md", section);
            linked.sort();
            let mut top: Vec<String> = collections
                .iter()
                .filter(|c| !c.path.contains('/'))
                .map(|c| format!("{}/_collection.md", c.path))
                .collect();
            top.sort();
            if linked != top {
                problems.add(format!(
                    "ARCHIVE.md: collections section links {linked:?}, not the top-level collections {top:?}"
                ));
            }
        }
        _ => problems.add("ARCHIVE.md: no collections section"),
    }

    for c in collections {
        let doc = format!("{}/_collection.md", c.path);
        let text = read(&doc);
        let Ok(Some(listing)) = sections::get(&text, "listing") else {
            problems.add(format!("{doc}: no listing section"));
            continue;
        };
        let links = links_from(&doc, listing);
        let parent = if c.path.contains('/') {
            format!("{}/_collection.md", parent_of(&c.path))
        } else {
            "ARCHIVE.md".to_owned()
        };
        if !listing.contains("Part of [") || links.first() != Some(&parent) {
            problems.add(format!(
                "{doc}: listing doesn't start with \"Part of\" {parent}"
            ));
        }
        let expected: BTreeSet<String> = collections
            .iter()
            .filter(|s| parent_of(&s.path) == c.path)
            .map(|s| format!("{}/_collection.md", s.path))
            .chain(
                items
                    .iter()
                    .filter(|i| parent_of(&i.card) == c.path)
                    .map(|i| i.card.clone()),
            )
            .collect();
        let listed: BTreeSet<String> = links.into_iter().skip(1).collect();
        for missing in expected.difference(&listed) {
            problems.add(format!("{doc}: listing doesn't link {missing}"));
        }
        for extra in listed.difference(&expected) {
            problems.add(format!(
                "{doc}: listing links {extra}, which isn't its subcollection or card"
            ));
        }
    }

    // Everything can be reached from ARCHIVE.md by following links.
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from(["ARCHIVE.md".to_owned()]);
    while let Some(path) = queue.pop_front() {
        if !has_extension(&path, "md") || !root.join(&path).is_file() || !seen.insert(path.clone())
        {
            continue;
        }
        queue.extend(links_from(&path, &read(&path)));
    }
    let unreached: Vec<String> = collections
        .iter()
        .map(|c| format!("{}/_collection.md", c.path))
        .chain(items.iter().map(|i| i.card.clone()))
        .filter(|p| !seen.contains(p))
        .collect();
    if !unreached.is_empty() {
        problems.add(format!("not reachable from ARCHIVE.md: {unreached:?}"));
    }
}

fn check_system_files(root: &Path, problems: &mut Problems) {
    let mut files = Vec::new();
    files_under(root, &root.join("_System"), &mut files);
    for file in files {
        let Ok(text) = std::fs::read_to_string(root.join(&file)) else {
            continue;
        };
        if has_extension(&file, "json") && serde_json::from_str::<Value>(&text).is_err() {
            problems.add(format!("{file}: invalid JSON"));
        }
        if has_extension(&file, "yaml") && serde_saphyr::from_str::<Value>(&text).is_err() {
            problems.add(format!("{file}: invalid YAML"));
        }
    }
    let spec = std::fs::read(repo_root().join("FORMAT.md")).unwrap();
    if std::fs::read(root.join("_System/FORMAT.md")).ok().as_ref() != Some(&spec) {
        problems.add("_System/FORMAT.md isn't an exact copy of FORMAT.md");
    }
}

fn check_review_entries(
    archive: &Archive,
    collections: &[carrel_core::archive::Collection],
    problems: &mut Problems,
) {
    let queue = carrel_core::review::ReviewQueue::of(archive);
    let (entries, bad) = queue.list().unwrap();
    for error in bad {
        problems.add(format!("review entry: {error}"));
    }
    for entry in entries {
        let name = format!("{}/{}.json", carrel_core::review::REVIEW_DIR, entry.id);
        let Some(collection) = collections
            .iter()
            .find(|c| c.doc.id == entry.destination.collection)
        else {
            problems.add(format!(
                "{name}: destination {} isn't a collection",
                entry.destination.collection
            ));
            continue;
        };
        if collection.path != entry.destination.path {
            problems.add(format!(
                "{name}: destination path {} doesn't match {}",
                entry.destination.path, collection.path
            ));
        }
        let mut vocabulary: BTreeSet<&str> = archive
            .doc()
            .tags
            .iter()
            .map(|t| t.name.0.as_str())
            .collect();
        for c in collections {
            if collection.path == c.path || collection.path.starts_with(&format!("{}/", c.path)) {
                vocabulary.extend(c.doc.tags.iter().map(|t| t.name.0.as_str()));
            }
        }
        for tag in &entry.card.tags {
            if !vocabulary.contains(tag.0.as_str()) && !entry.proposed_tags.contains(tag) {
                problems.add(format!(
                    "{name}: tag {} is neither in the vocabulary nor proposed",
                    tag.0
                ));
            }
        }
        for tag in &entry.proposed_tags {
            if vocabulary.contains(tag.0.as_str()) {
                problems.add(format!(
                    "{name}: proposed tag {} is already in the vocabulary",
                    tag.0
                ));
            }
        }
    }
}

#[test]
fn the_sample_archive_conforms() {
    let problems = check(&sample_archive());
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// A copy of the sample archive, changed by `edit`, and the problems found in it.
fn problems_after(edit: impl FnOnce(&Path)) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&sample_archive(), dir.path());
    edit(dir.path());
    check(dir.path())
}

fn replace_in(path: &Path, from: &str, to: &str) {
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains(from), "{} has no {from:?}", path.display());
    std::fs::write(path, text.replacen(from, to, 1)).unwrap();
}

fn assert_found(problems: &[String], expected: &str) {
    assert!(
        problems.iter().any(|p| p.contains(expected)),
        "expected a problem containing {expected:?}, got:\n{}",
        problems.join("\n")
    );
}

#[test]
fn changed_items_are_caught() {
    let problems = problems_after(|root| {
        std::fs::write(root.join("Projects/Hardware/parts-list.csv"), "changed\n").unwrap();
        std::fs::write(root.join("Projects/oscillator-sim/extra.txt"), "new\n").unwrap();
    });
    assert_found(&problems, "parts-list.csv.md: checksum doesn't match");
    assert_found(&problems, "parts-list.csv.md: size doesn't match");
    assert_found(&problems, "oscillator-sim.md: manifest is out of date");
}

#[test]
fn schema_violations_are_caught() {
    let problems = problems_after(|root| {
        replace_in(
            &root.join("Reading/On the Origin of Species.md"),
            "sensitivity: public",
            "sensitivity: secret",
        );
    });
    assert_found(&problems, "On the Origin of Species.md: schema:");
}

#[test]
fn broken_links_and_navigation_are_caught() {
    let problems = problems_after(|root| {
        replace_in(
            &root.join("Projects/oscillator-sim.md"),
            "path: \"../Reading/Coupled oscillators note.pdf.md\"",
            "path: \"../Reading/elsewhere.md\"",
        );
        let listing = root.join("Reading/_collection.md");
        let text = std::fs::read_to_string(&listing).unwrap();
        let kept: String = text
            .lines()
            .filter(|l| !l.contains("On%20the%20Origin"))
            .flat_map(|l| [l, "\n"])
            .collect();
        assert_ne!(kept, text, "the listing should link the book's card");
        std::fs::write(&listing, kept).unwrap();
    });
    assert_found(
        &problems,
        "oscillator-sim.md: link path ../Reading/elsewhere.md",
    );
    assert_found(
        &problems,
        "Reading/_collection.md: listing doesn't link Reading/On the Origin of Species.md",
    );
}

#[test]
fn uncataloged_files_and_duplicate_ids_are_caught() {
    let problems = problems_after(|root| {
        std::fs::write(root.join("Reading/stray.txt"), "?\n").unwrap();
        std::fs::write(root.join("loose.txt"), "?\n").unwrap();
        let book =
            std::fs::read_to_string(root.join("Reading/On the Origin of Species.md")).unwrap();
        let id_line = book
            .lines()
            .find(|l| l.starts_with("id: "))
            .unwrap()
            .to_owned();
        let other = root.join("Projects/oscillator-sim.md");
        let text = std::fs::read_to_string(&other).unwrap();
        let old = text
            .lines()
            .find(|l| l.starts_with("id: "))
            .unwrap()
            .to_owned();
        std::fs::write(&other, text.replacen(&old, &id_line, 1)).unwrap();
    });
    assert_found(&problems, "uncataloged: Reading/stray.txt");
    assert_found(&problems, "uncataloged at the root: loose.txt");
    assert_found(&problems, "is also used by");
}

#[test]
fn a_stale_spec_copy_is_caught() {
    let problems = problems_after(|root| {
        std::fs::write(root.join("_System/FORMAT.md"), "# Old\n").unwrap();
    });
    assert_found(&problems, "_System/FORMAT.md isn't an exact copy");
}
