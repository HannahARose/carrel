//! A STEP file's header, and the names of its parts, found by scanning for `PRODUCT(` without
//! parsing any geometry. Files are often tens of megabytes, so they're streamed.

use std::io::Read;
use std::path::Path;

use super::{Details, date, push_unique};

/// How much of the start is searched for the header, in bytes.
const HEADER_BYTES: usize = 64 * 1024;

/// How much is read at a time while looking for parts.
const CHUNK: usize = 1024 * 1024;

/// More than `PRODUCT` and any spaces before its `(`.
const KEYWORD: usize = 64;

/// How much of each chunk is kept for the next, so a part split between chunks is found: more
/// than any `PRODUCT(` entity's start.
const OVERLAP: usize = 4096;

pub(super) fn read(path: &Path) -> std::io::Result<Details> {
    let mut file = std::fs::File::open(path)?;
    let mut details = Details::new("CAD model (STEP)");
    let mut parts = Vec::new();
    let mut buffer = Vec::with_capacity(CHUNK + OVERLAP);
    let mut chunk = vec![0; CHUNK];
    let mut first = true;
    loop {
        let read = file.read(&mut chunk)?;
        buffer.extend_from_slice(&chunk[..read]);
        if first && (buffer.len() >= HEADER_BYTES || read == 0) {
            header(
                &String::from_utf8_lossy(&buffer[..buffer.len().min(HEADER_BYTES)]),
                &mut details,
            );
            first = false;
        }
        if read == 0 {
            products(&buffer, buffer.len(), &mut parts);
            break;
        }
        if !first && buffer.len() > OVERLAP {
            let end = buffer.len() - OVERLAP;
            products(&buffer, end, &mut parts);
            // A part whose `(` is just past `end` is found next time; one just before it may be
            // found twice, which `push_unique` allows for.
            buffer.drain(..end - KEYWORD);
        }
    }
    details.add_names("Parts", &parts);
    Ok(details)
}

/// The header's `FILE_DESCRIPTION`, `FILE_NAME` and `FILE_SCHEMA`.
fn header(text: &str, details: &mut Details) {
    let header = text.find("ENDSEC").map_or(text, |end| &text[..end]);
    if let Some(schema) = entity(header, "FILE_SCHEMA").and_then(|f| strings(f).into_iter().next())
    {
        let protocol = schema.split_whitespace().next().unwrap_or_default();
        // The application protocols' numbers, which the older schemas' names don't give.
        let short = match protocol.to_ascii_uppercase().as_str() {
            "AUTOMOTIVE_DESIGN" => "AP214",
            "CONFIG_CONTROL_DESIGN" => "AP203",
            _ => protocol.split('_').next().unwrap_or(protocol),
        };
        details.heading = format!("CAD model (STEP, {short})");
    }
    if let Some(fields) = entity(header, "FILE_NAME") {
        // name, time stamp, (authors), (organisations), preprocessor, originating system,
        // authorisation
        let values = strings(fields);
        let get = |i: usize| values.get(i).map(String::as_str).unwrap_or_default();
        let groups = groups(fields);
        let group = |i: usize| groups.get(i).map(|g| g.join(", ")).unwrap_or_default();
        details.add("Name", get(0));
        details.created = date(get(1));
        let system = if get(5).trim().is_empty() || get(5).eq_ignore_ascii_case("unknown") {
            get(4)
        } else {
            get(5)
        };
        let mut exported = details.created.clone().unwrap_or_default();
        if !system.trim().is_empty() {
            exported = if exported.is_empty() {
                format!("from {}", system.trim())
            } else {
                format!("{exported} from {}", system.trim())
            };
        }
        details.add("Exported", exported);
        details.add("Author", group(0));
        details.add("Organisation", group(1));
    }
    if let Some(description) = entity(header, "FILE_DESCRIPTION") {
        let lines: Vec<String> = groups(description)
            .into_iter()
            .next()
            .unwrap_or_default()
            .into_iter()
            .filter(|l| !boilerplate(l))
            .collect();
        details.add("Description", lines.join(" "));
    }
}

/// Whether a description line only says which conventions the exporter followed, such as
/// `CAx-IF Rec.Pracs.---3D Tessellated Geometry---0.4---2014-09-14` or `2;1`.
fn boilerplate(line: &str) -> bool {
    let line = line.trim();
    line.is_empty()
        || line.eq_ignore_ascii_case("unknown")
        || line.contains("CAx-IF")
        || line.starts_with("STEP AP")
        || line.chars().all(|c| c.is_ascii_digit() || c == ';')
}

/// The text inside `NAME( … )`, up to its closing `;`.
fn entity<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let start = find_entity(text.as_bytes(), name.as_bytes(), 0)?;
    let rest = &text[start..];
    let end = statement_end(rest);
    Some(&rest[..end])
}

/// Where `name`, a whole word followed by `(` after any spaces, starts at or after `from`; the
/// position just after the `(`.
fn find_entity(text: &[u8], name: &[u8], from: usize) -> Option<usize> {
    let mut at = from;
    while let Some(found) = text[at..]
        .windows(name.len())
        .position(|w| w == name)
        .map(|i| at + i)
    {
        at = found + name.len();
        let before = found.checked_sub(1).map(|i| text[i]);
        if before.is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_') {
            continue;
        }
        let mut next = at;
        while text.get(next).is_some_and(u8::is_ascii_whitespace) {
            next += 1;
        }
        if text.get(next) == Some(&b'(') {
            return Some(next + 1);
        }
    }
    None
}

/// The length of the statement at the start of `text`: up to the first `;` outside a string.
fn statement_end(text: &str) -> usize {
    let mut quoted = false;
    for (i, c) in text.char_indices() {
        match c {
            '\'' => quoted = !quoted,
            ';' if !quoted => return i,
            _ => {}
        }
    }
    text.len()
}

/// Every string in `text`, in order, decoded.
fn strings(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('\'') {
        let (value, after) = string(&rest[start + 1..]);
        out.push(value);
        rest = after;
    }
    out
}

/// The strings in each parenthesised list in `text`, such as the authors in `FILE_NAME`.
fn groups(text: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut rest = text;
    loop {
        let next_quote = rest.find('\'');
        let next_open = rest.find('(');
        match (next_quote, next_open) {
            (Some(q), Some(o)) if q < o => {
                rest = string(&rest[q + 1..]).1;
            }
            (_, Some(o)) => {
                let inner = &rest[o + 1..];
                let close = closing(inner);
                out.push(strings(&inner[..close]));
                rest = &inner[close.min(inner.len())..];
            }
            _ => return out,
        }
    }
}

/// Where the list starting at `text` closes, skipping strings.
fn closing(text: &str) -> usize {
    let mut quoted = false;
    for (i, c) in text.char_indices() {
        match c {
            '\'' => quoted = !quoted,
            ')' if !quoted => return i,
            _ => {}
        }
    }
    text.len()
}

/// A string's value, from just after its opening quote, and the text after it. `''` is a
/// quote, and `\X2\…\X0\` and `\X\hh` are the encodings of other characters.
fn string(text: &str) -> (String, &str) {
    let mut raw = String::new();
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '\'' {
            if chars.peek().is_some_and(|(_, n)| *n == '\'') {
                chars.next();
                raw.push('\'');
                continue;
            }
            return (decode(&raw), &text[i + 1..]);
        }
        // Long strings are wrapped across lines; the line breaks aren't part of them.
        if c != '\n' && c != '\r' {
            raw.push(c);
        }
    }
    (decode(&raw), "")
}

/// STEP's encodings: `\X2\00E9\X0\` (UTF-16), `\X\E9` (ISO 8859-1), `\\` (a backslash).
fn decode(raw: &str) -> String {
    if !raw.contains('\\') {
        return raw.to_owned();
    }
    let mut out = String::new();
    let mut rest = raw;
    while let Some(start) = rest.find('\\') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        if let Some(body) = tail.strip_prefix("\\X2\\") {
            let end = body.find("\\X0\\").unwrap_or(body.len());
            let units: Vec<u16> = body.as_bytes()[..end]
                .chunks(4)
                .filter_map(|c| u16::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok())
                .collect();
            out.push_str(&String::from_utf16_lossy(&units));
            rest = body.get(end + 4..).unwrap_or_default();
        } else if let Some(hex) = tail.strip_prefix("\\X\\").and_then(|t| t.get(..2))
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(char::from(byte));
            rest = &tail[5..];
        } else if let Some(after) = tail.strip_prefix("\\\\") {
            out.push('\\');
            rest = after;
        } else {
            out.push('\\');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

/// The names of the `PRODUCT(` entities that start before `end`.
fn products(buffer: &[u8], end: usize, parts: &mut Vec<String>) {
    let mut at = 0;
    while let Some(open) = find_entity(buffer, b"PRODUCT", at) {
        if open > end {
            break;
        }
        at = open;
        let rest = String::from_utf8_lossy(&buffer[open..buffer.len().min(open + OVERLAP)]);
        let fields = &rest[..statement_end(&rest)];
        // PRODUCT('id', 'name', 'description', (contexts)): the name, else the identifier.
        let values = strings(fields);
        let name = values
            .get(1)
            .filter(|n| !n.trim().is_empty())
            .or_else(|| values.first());
        if let Some(name) = name {
            // SolidWorks names an assembly's parts `part^assembly`.
            push_unique(parts, name.split('^').next().unwrap_or(name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONSHAPE: &str = "ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('STEP AP242','CAx-IF Rec.Pracs.---3D Tessellated Geometry---0.4---2014-09-14','2;1','Sample holder for the
 OLED rig'),'2;1');
FILE_NAME('sample holder','2026-10-11T14:22:05',('R. Lee'),(''),'Onshape','Onshape','');
FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }'));
ENDSEC;
DATA;
#1 = PRODUCT_DEFINITION('design','',#2,#3);
#10 = PRODUCT('Left OLED Holder','Left OLED Holder','',(#5));
#11=PRODUCT ( 'Rear Body' , '' , '' , (#5) ) ;
#12 = PRODUCT('M\\X2\\00FC\\X0\\ller''s part','M\\X2\\00FC\\X0\\ller''s part','',(#5));
#13 = PRODUCT('Left OLED Holder^Holder assembly','Left OLED Holder^Holder assembly','',(#5));
ENDSEC;
END-ISO-10303-21;
";

    fn write(text: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("part.step");
        std::fs::write(&path, text).unwrap();
        (dir, path)
    }

    #[test]
    fn the_header_and_part_names_are_read() {
        let (_dir, path) = write(ONSHAPE.as_bytes());
        let details = read(&path).unwrap();
        assert_eq!(
            details.text(),
            "CAD model (STEP, AP242)
Name: sample holder
Exported: 2026-10-11 from Onshape
Author: R. Lee
Description: Sample holder for the OLED rig
Parts: Left OLED Holder, Rear Body, Müller's part"
        );
        assert_eq!(details.created.as_deref(), Some("2026-10-11"));
    }

    #[test]
    fn solidworks_names_its_system_and_organisation() {
        let (_dir, path) = write(
            b"ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION (( 'STEP AP214' ),\n    '1' );\nFILE_NAME ('OBIS Laser.STEP',\n    '2012-01-18T10:32:17',\n    ( '' ),\n    ( 'coherent' ),\n    'SwSTEP 2.0',\n    'SolidWorks 2012',\n    '' );\nFILE_SCHEMA (( 'AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }' ));\nENDSEC;\n",
        );
        let text = read(&path).unwrap().text();
        assert!(
            text.contains("Exported: 2012-01-18 from SolidWorks 2012"),
            "{text}"
        );
        assert!(text.contains("Organisation: coherent"), "{text}");
        assert!(!text.contains("Author"), "{text}");
        assert!(text.starts_with("CAD model (STEP, AP214)\n"), "{text}");
    }

    #[test]
    fn parts_are_found_across_chunks_of_a_large_file() {
        let mut text = ONSHAPE.replace("ENDSEC;\nEND", "").into_bytes();
        text.extend(std::iter::repeat_n(b'x', CHUNK - text.len() - 20));
        text.extend_from_slice(b"\n#99 = PRODUCT('Split across chunks','','',(#5));\n");
        text.extend(std::iter::repeat_n(b'y', CHUNK * 2));
        text.extend_from_slice(b"\n#100 = PRODUCT('At the end','','',(#5));\nENDSEC;\n");
        let (_dir, path) = write(&text);
        let text = read(&path).unwrap().text();
        assert!(
            text.ends_with("Müller's part, Split across chunks, At the end"),
            "{text}"
        );
    }
}
