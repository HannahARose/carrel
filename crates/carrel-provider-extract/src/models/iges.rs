//! An IGES file's start and global sections: 80-column lines, with the section's letter in
//! column 73, at the start of the file.

use std::io::{BufRead, BufReader};
use std::path::Path;

use super::Details;

/// The most lines read looking for the start and global sections.
const MAX_LINES: usize = 2000;

pub(super) fn read(path: &Path) -> std::io::Result<Details> {
    let reader = BufReader::new(std::fs::File::open(path)?);
    let mut start = String::new();
    let mut global = String::new();
    for line in reader.split(b'\n').take(MAX_LINES) {
        let line = line?;
        let line = String::from_utf8_lossy(&line);
        let line = line.trim_end_matches('\r');
        let (Some(body), Some(section)) = (line.get(..72), line.get(72..73)) else {
            continue;
        };
        match section {
            "S" => {
                let text = body.trim();
                if !text.is_empty() {
                    if !start.is_empty() {
                        start.push(' ');
                    }
                    start.push_str(text);
                }
            }
            "G" => global.push_str(body),
            _ => break,
        }
    }
    let mut details = Details::new("CAD model (IGES)");
    details.add("Start", &start);
    let fields = parameters(&global);
    let get = |n: usize| fields.get(n - 1).map(String::as_str).unwrap_or_default();
    // The global section's fields, numbered as in the standard.
    let name = if get(4).trim().is_empty() {
        get(3)
    } else {
        get(4)
    };
    details.add("Name", name);
    details.created = date(get(18));
    let system = get(5);
    let exported = match (&details.created, system.trim().is_empty()) {
        (Some(day), false) => format!("{day} from {}", system.trim()),
        (Some(day), true) => day.clone(),
        (None, false) => format!("from {}", system.trim()),
        (None, true) => String::new(),
    };
    details.add("Exported", exported);
    details.add("Author", get(21));
    details.add("Organisation", get(22));
    details.add("Units", get(15));
    Ok(details)
}

/// The global section's parameters: separated by its first field's delimiter (a comma by
/// default), ending at its second's (`;`), with strings in Hollerith form (`5HHello`).
fn parameters(global: &str) -> Vec<String> {
    let chars: Vec<char> = global.chars().collect();
    let mut delimiter = ',';
    let mut end = ';';
    let mut fields: Vec<String> = Vec::new();
    let mut at = 0;
    let mut field = String::new();
    while at < chars.len() {
        let c = chars[at];
        if c == delimiter || c == end {
            fields.push(std::mem::take(&mut field).trim().to_owned());
            // The first two fields may change the delimiters.
            if let (1 | 2, Some(set)) = (fields.len(), fields.last().and_then(|f| f.chars().next()))
                && fields.last().is_some_and(|f| f.chars().count() == 1)
            {
                if fields.len() == 1 {
                    delimiter = set;
                } else {
                    end = set;
                }
            }
            if c == end && fields.len() > 2 {
                break;
            }
            at += 1;
            continue;
        }
        if c.is_ascii_digit() && field.trim().is_empty() {
            // A count followed by `H` starts a string of that many characters.
            let digits: String = chars[at..]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if chars.get(at + digits.len()) == Some(&'H')
                && let Ok(count) = digits.parse::<usize>()
            {
                let from = at + digits.len() + 1;
                let to = (from + count).min(chars.len());
                field = chars[from..to].iter().collect();
                at = to;
                continue;
            }
        }
        field.push(c);
        at += 1;
    }
    if !field.trim().is_empty() {
        fields.push(field.trim().to_owned());
    }
    fields
}

/// The date of `20221201.120000` or `221201.120000` (IGES's older two-digit years).
fn date(text: &str) -> Option<String> {
    let digits = text.split('.').next()?.trim();
    let (year, rest) = match digits.len() {
        8 => (digits[..4].to_owned(), &digits[4..]),
        6 => {
            let short: u32 = digits[..2].parse().ok()?;
            (
                format!(
                    "{}",
                    if short < 70 {
                        2000 + short
                    } else {
                        1900 + short
                    }
                ),
                &digits[2..],
            )
        }
        _ => return None,
    };
    super::date(&format!("{year}-{}-{}", &rest[..2], &rest[2..4]))
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    /// IGES lines: the text in 72 columns, then the section and a sequence number.
    fn lines(section: char, texts: &[&str]) -> String {
        let mut out = String::new();
        for (i, text) in texts.iter().enumerate() {
            writeln!(out, "{text:<72}{section}{:>7}", i + 1).unwrap();
        }
        out
    }

    #[test]
    fn the_start_and_global_sections_are_read() {
        let file = lines('S', &["Bracket for the bench power supply", "Rev B"])
            + &lines(
                'G',
                &[
                    "1H,,1H;,7Hbracket,11Hbracket.igs,15HSolidWorks 2022,9HSwIGES 1.0,",
                    "32,308,15,308,15,7Hbracket,1.,2,2HMM,50,0.125,15H20221201.120000,",
                    "1E-08,100.,5HR Lee,8Hcoherent,11,0,15H20221201.115900;",
                ],
            )
            + &lines('D', &["     110       1       0       0       0"]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bracket.igs");
        std::fs::write(&path, file).unwrap();
        let details = read(&path).unwrap();
        assert_eq!(
            details.text(),
            "CAD model (IGES)
Start: Bracket for the bench power supply Rev B
Name: bracket.igs
Exported: 2022-12-01 from SolidWorks 2022
Author: R Lee
Organisation: coherent
Units: MM"
        );
        assert_eq!(details.created.as_deref(), Some("2022-12-01"));
    }

    #[test]
    fn two_digit_years_are_read() {
        assert_eq!(date("991231.235959").as_deref(), Some("1999-12-31"));
        assert_eq!(date("050102.000000").as_deref(), Some("2005-01-02"));
        assert_eq!(date(""), None);
    }
}
