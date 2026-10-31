//! A 3MF file: zipped XML from a slicer or CAD program, with metadata (title, designer,
//! description, license), the slicer's settings (printer, filament), and the objects' names.
//! The meshes are streamed, only to count their triangles.

use std::fmt::Write as _;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use carrel_core::providers::ProviderError;
use zip::ZipArchive;

use super::{Details, date, duration, push_unique};

/// How much of the main model is searched for metadata, which comes before the meshes.
const METADATA_BYTES: u64 = 1024 * 1024;

/// The longest description kept, in characters.
const MAX_DESCRIPTION: usize = 2000;

/// The metadata shown, by its name in the file, and its label.
const METADATA: [(&str, &str); 6] = [
    ("Title", "Title"),
    ("Designer", "Designer"),
    ("Description", "Description"),
    ("License", "License"),
    ("LicenseTerms", "License"),
    ("Copyright", "Copyright"),
];

pub(super) fn read(path: &Path) -> Result<Details, ProviderError> {
    let failed =
        |e: &dyn std::fmt::Display| ProviderError::Failed(format!("{}: {e}", path.display()));
    let file = File::open(path).map_err(|e| failed(&e))?;
    let mut archive = ZipArchive::new(file).map_err(|e| failed(&e))?;
    let mut details = Details::new("3D print project (3MF)");

    let root = root_model(&mut archive);
    if let Ok(entry) = archive.by_name(&root) {
        let mut head = String::new();
        let _ = entry.take(METADATA_BYTES).read_to_string(&mut head);
        metadata(&head, &mut details);
    }
    settings(&mut archive, &mut details);

    let mut objects = Vec::new();
    let mut triangles = 0u64;
    let models: Vec<String> = archive
        .file_names()
        .filter_map(Result::ok)
        .filter(|n| n.to_ascii_lowercase().ends_with(".model"))
        .map(std::borrow::Cow::into_owned)
        .collect();
    for name in models {
        if let Ok(entry) = archive.by_name(&name) {
            triangles += scan(entry, &mut objects).map_err(|e| failed(&e))?;
        }
    }
    if let Some(text) = text_of(&mut archive, "Metadata/model_settings.config") {
        // Bambu Studio names objects here: `<metadata key="name" value="…"/>`.
        for tag in text.split('<').filter(|t| t.starts_with("metadata ")) {
            if attribute(tag, "key").as_deref() == Some("name")
                && let Some(name) = attribute(tag, "value")
            {
                push_unique(&mut objects, &name);
            }
        }
    }
    for name in used(&mut archive).objects {
        push_unique(&mut objects, &name);
    }
    details.add_names("Objects", &objects);
    let plates = archive
        .file_names()
        .filter_map(Result::ok)
        .filter(|n| n.starts_with("Metadata/plate_") && n.ends_with(".gcode"))
        .count();
    if plates > 0 {
        details.add(
            "Sliced",
            format!(
                "G-code for {plates} plate{}",
                if plates == 1 { "" } else { "s" }
            ),
        );
    }
    if triangles > 0 {
        details.add("Triangles", grouped(triangles));
    }
    Ok(details)
}

/// The main model's path, from the package's relationships, or where it usually is.
fn root_model(archive: &mut ZipArchive<File>) -> String {
    let rels = text_of(archive, "_rels/.rels").unwrap_or_default();
    rels.split('<')
        .filter(|t| t.starts_with("Relationship "))
        .find(|t| attribute(t, "Type").is_some_and(|k| k.ends_with("/3dmodel")))
        .and_then(|t| attribute(t, "Target"))
        .map_or_else(
            || "3D/3dmodel.model".to_owned(),
            |target| target.trim_start_matches('/').to_owned(),
        )
}

/// The `<metadata name="…">…</metadata>` elements before the meshes.
fn metadata(head: &str, details: &mut Details) {
    let mut application = None;
    for element in head.split("<metadata ").skip(1) {
        let Some((tag, rest)) = element.split_once('>') else {
            continue;
        };
        let Some(name) = attribute(tag, "name") else {
            continue;
        };
        let value = rest.split("</metadata>").next().unwrap_or_default();
        let value = plain(&unescape(value));
        if let Some((_, label)) = METADATA.iter().find(|(n, _)| n.eq_ignore_ascii_case(&name)) {
            let value: String = value.chars().take(MAX_DESCRIPTION).collect();
            details.add(label, value);
        } else if name.eq_ignore_ascii_case("CreationDate") {
            details.created = date(&value);
        } else if name.eq_ignore_ascii_case("Application") {
            application = Some(value);
        }
    }
    if let Some(application) = application.filter(|a| !a.trim().is_empty()) {
        details.heading = format!("3D print project (3MF), from {}", application.trim());
    }
}

/// The slicer's printer, filament and layer height: `PrusaSlicer`'s settings as `; key = value`
/// lines, Bambu Studio's and `OrcaSlicer`'s as JSON.
fn settings(archive: &mut ZipArchive<File>, details: &mut Details) {
    if let Some(text) = text_of(archive, "Metadata/Slic3r_PE.config") {
        let settings = super::gcode::details(&text);
        for (label, value) in settings.lines {
            if label != "Objects" {
                details.add(label, value);
            }
        }
        return;
    }
    if let Some(text) = text_of(archive, "Metadata/project_settings.config") {
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
            return;
        };
        // Lists have one value per filament; the first is enough to say what it's printed in.
        let get = |key: &str| match &json[key] {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(a) => a
                .first()
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            _ => String::new(),
        };
        details.add("Printer", get("printer_settings_id"));
        let used = used(archive);
        let mut filament = get("filament_settings_id");
        let missing: Vec<&str> = used
            .types
            .iter()
            .map(String::as_str)
            .filter(|t| !filament.contains(t))
            .collect();
        if !missing.is_empty() {
            write!(filament, " ({})", missing.join(", ")).unwrap();
        }
        if let Some(grams) = used.grams.filter(|g| !filament.is_empty() && g != "0") {
            write!(filament, ", {grams} g").unwrap();
        }
        details.add("Filament", filament);
        let layer = get("layer_height");
        if !layer.is_empty() {
            details.add("Layer height", format!("{layer} mm"));
        }
        if let Some(seconds) = used.seconds {
            details.add("Print time", duration(seconds));
        }
    }
}

/// What Bambu Studio records of a sliced plate in `Metadata/slice_info.config`.
#[derive(Debug, Default)]
struct Used {
    /// The filament types used, such as `PETG`.
    types: Vec<String>,
    /// How much filament, in grams.
    grams: Option<String>,
    /// How long it takes to print, in seconds.
    seconds: Option<u64>,
    /// The objects on the plate.
    objects: Vec<String>,
}

fn used(archive: &mut ZipArchive<File>) -> Used {
    let mut used = Used::default();
    let text = text_of(archive, "Metadata/slice_info.config").unwrap_or_default();
    for tag in text.split('<') {
        if tag.starts_with("object ") {
            if let Some(name) = attribute(tag, "name") {
                push_unique(&mut used.objects, &name);
            }
        } else if tag.starts_with("filament ") {
            if let Some(kind) = attribute(tag, "type") {
                push_unique(&mut used.types, &kind);
            }
        } else if tag.starts_with("metadata ") {
            match attribute(tag, "key").as_deref() {
                Some("prediction") => {
                    used.seconds = attribute(tag, "value").and_then(|v| v.parse().ok());
                }
                Some("weight") => used.grams = attribute(tag, "value"),
                _ => {}
            }
        }
    }
    used
}

/// Counts the triangles in a model, and adds its objects' names, reading it in pieces.
fn scan(mut entry: impl Read, objects: &mut Vec<String>) -> std::io::Result<u64> {
    const CHUNK: usize = 256 * 1024;
    const OVERLAP: usize = 1024;
    let mut buffer = Vec::with_capacity(CHUNK + OVERLAP);
    let mut chunk = vec![0; CHUNK];
    let mut triangles = 0;
    loop {
        let read = entry.read(&mut chunk)?;
        buffer.extend_from_slice(&chunk[..read]);
        // Tags that start before `end` are counted now; the rest wait for the next piece.
        let end = if read == 0 {
            buffer.len()
        } else {
            buffer.len().saturating_sub(OVERLAP)
        };
        let mut at = 0;
        while let Some(i) = find(&buffer[at..end], b"<triangle") {
            let after = buffer.get(at + i + 9).copied();
            if after.is_some_and(|b| b.is_ascii_whitespace() || b == b'/') {
                triangles += 1;
            }
            at += i + 9;
        }
        at = 0;
        while let Some(i) = find(&buffer[at..end], b"<object ") {
            let tag_end = buffer[at + i..].iter().position(|&b| b == b'>');
            let tag = String::from_utf8_lossy(&buffer[at + i..at + i + tag_end.unwrap_or(0)]);
            if let Some(name) = attribute(&tag, "name") {
                push_unique(objects, &name);
            }
            at += i + 8;
        }
        if read == 0 {
            return Ok(triangles);
        }
        buffer.drain(..end);
    }
}

/// The text of the file `name` in the archive, when it's there.
fn text_of(archive: &mut ZipArchive<File>, name: &str) -> Option<String> {
    let mut text = String::new();
    archive.by_name(name).ok()?.read_to_string(&mut text).ok()?;
    Some(text)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// The value of `name="…"` in a tag, unescaped.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let mut rest = tag;
    while let Some(i) = rest.find(name) {
        let before = rest[..i].chars().next_back();
        let after = rest[i + name.len()..].trim_start();
        rest = &rest[i + name.len()..];
        if before.is_some_and(|c| !c.is_whitespace()) {
            continue;
        }
        let Some(value) = after.strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        let quote = value.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let end = value[1..].find(quote)?;
        return Some(unescape(&value[1..=end]));
    }
    None
}

/// XML's entities as characters.
fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        if let Some(c) = decoded {
            out.push(c);
            rest = &rest[end + 1..];
        } else {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

/// Text without HTML tags, with its spaces collapsed: model sites' descriptions are HTML.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `10220` as `10,220`.
fn grouped(number: u64) -> String {
    let digits = number.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use super::*;

    /// A 3MF with these files in it.
    pub(crate) fn write_3mf(dir: &Path, files: &[(&str, &[u8])]) -> std::path::PathBuf {
        let path = dir.join("plate.3mf");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        for (name, bytes) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    pub(crate) const MODEL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
 <metadata name="Title">Curtain glider &amp; hook</metadata>
 <metadata name="Designer">R. Lee</metadata>
 <metadata name="Description">&lt;p&gt;Replacement gliders for a &lt;b&gt;curtain&lt;/b&gt; track.&lt;/p&gt;</metadata>
 <metadata name="License">CC BY 4.0</metadata>
 <metadata name="CreationDate">2026-10-02</metadata>
 <metadata name="Application">BambuStudio-01.09.00.70</metadata>
 <resources>
  <object id="1" name="glider" type="model"><mesh><vertices>
   <vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/>
  </vertices><triangles>
   <triangle v1="0" v2="1" v3="2"/><triangle v1="0" v2="2" v3="1"/>
  </triangles></mesh></object>
  <object id="2" name="hook" type="model"><mesh><triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object>
 </resources>
 <build><item objectid="1"/></build>
</model>"#;

    const SLICE_INFO: &str = r#"<config><plate>
    <metadata key="prediction" value="3786"/>
    <metadata key="weight" value="10.21"/>
    <object identify_id="212" name="glider inlay" skipped="false" />
    <filament id="1" type="PLA" used_g="6.37"/>
    <filament id="2" type="PETG" used_g="3.85"/>
    </plate></config>"#;

    #[test]
    fn metadata_settings_and_objects_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let settings = br#"{"printer_settings_id": "Bambu Lab X1 Carbon 0.4 nozzle",
            "filament_settings_id": ["Bambu PLA Basic @BBL X1C", "Bambu PETG"],
            "filament_type": ["PLA", "PETG"], "layer_height": "0.2"}"#;
        let path = write_3mf(
            dir.path(),
            &[
                ("3D/3dmodel.model", MODEL.as_bytes()),
                ("Metadata/project_settings.config", settings),
                ("Metadata/slice_info.config", SLICE_INFO.as_bytes()),
                ("Metadata/plate_1.gcode", b"G28\n"),
            ],
        );
        let details = read(&path).unwrap();
        assert_eq!(
            details.text(),
            "3D print project (3MF), from BambuStudio-01.09.00.70
Title: Curtain glider & hook
Designer: R. Lee
Description: Replacement gliders for a curtain track.
License: CC BY 4.0
Printer: Bambu Lab X1 Carbon 0.4 nozzle
Filament: Bambu PLA Basic @BBL X1C (PETG), 10.21 g
Layer height: 0.2 mm
Print time: 1h 3m
Objects: glider, hook, glider inlay
Sliced: G-code for 1 plate
Triangles: 3"
        );
        assert_eq!(details.created.as_deref(), Some("2026-10-02"));
    }

    #[test]
    fn prusaslicer_settings_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let config = b"; filament_type = PETG\n; layer_height = 0.15\n; printer_settings_id = Original Prusa MK4\n";
        let path = write_3mf(
            dir.path(),
            &[
                ("3D/3dmodel.model", MODEL.as_bytes()),
                ("Metadata/Slic3r_PE.config", config),
            ],
        );
        let text = read(&path).unwrap().text();
        assert!(
            text.contains("Printer: Original Prusa MK4\nFilament: PETG\nLayer height: 0.15 mm"),
            "{text}"
        );
    }

    #[test]
    fn a_broken_archive_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.3mf");
        std::fs::write(&path, b"PK\x03\x04 not really").unwrap();
        assert!(matches!(read(&path), Err(ProviderError::Failed(_))));
    }

    #[test]
    fn helpers() {
        assert_eq!(grouped(10_220), "10,220");
        assert_eq!(grouped(999), "999");
        assert_eq!(
            unescape("a &#233; &#xE9; &bogus b & c"),
            "a é é &bogus b & c"
        );
        assert_eq!(
            attribute(r#"object id="1" name='a &amp; b'"#, "name").as_deref(),
            Some("a & b")
        );
        assert_eq!(
            attribute(r#"object filename="x" name="y""#, "name").as_deref(),
            Some("y")
        );
    }
}
