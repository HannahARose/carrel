//! The file pickers Carrel knows for choosing what to ingest, and the editor for cards: which
//! of each this device uses and why, for every interface's Settings (`docs/design/tui-ingest.md`).
//! The command line and the interactive mode run them; the app only shows and sets them.

use crate::providers::process::{default_folders, find_program};

/// What the `picker` preference names when the owner wants the built-in browser.
pub const BUILT_IN: &str = "builtin";

/// A file picker Carrel knows how to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Known {
    /// Its name, as in the `picker` preference.
    pub name: &'static str,
    /// The command, with `{out}` for the file to write chosen paths to; without it, the
    /// picker writes them to standard output.
    pub command: &'static str,
    /// What it's like, for Settings.
    pub about: &'static str,
    /// How to install it on macOS, Linux and Windows; `None` where it doesn't run.
    pub install: [Option<&'static str>; 3],
}

/// The pickers found automatically, in order of preference.
pub const KNOWN: [Known; 6] = [
    Known {
        name: "yazi",
        command: "yazi --chooser-file={out}",
        about: "fast, with previews of files and images",
        install: [
            Some("brew install yazi"),
            Some("cargo install --locked yazi-fm yazi-cli"),
            Some("winget install sxyazi.yazi"),
        ],
    },
    Known {
        name: "lf",
        command: "lf -selection-path={out}",
        about: "small and quick, with ranger's keys",
        install: [
            Some("brew install lf"),
            Some("apt install lf"),
            Some("scoop install lf"),
        ],
    },
    Known {
        name: "nnn",
        command: "nnn -p {out}",
        about: "tiny and fast, with many plugins",
        install: [Some("brew install nnn"), Some("apt install nnn"), None],
    },
    Known {
        name: "ranger",
        command: "ranger --choosefiles={out}",
        about: "Vim keys, three columns and previews",
        install: [
            Some("brew install ranger"),
            Some("apt install ranger"),
            None,
        ],
    },
    Known {
        name: "vifm",
        command: "vifm --choose-files {out}",
        about: "two panes, driven like Vim",
        install: [Some("brew install vifm"), Some("apt install vifm"), None],
    },
    Known {
        name: "fzf",
        command: "fzf --multi --walker=file,dir",
        about: "type to find files anywhere below the folder",
        install: [
            Some("brew install fzf"),
            Some("apt install fzf"),
            Some("winget install junegunn.fzf"),
        ],
    },
];

impl Known {
    /// Whether it's installed here.
    #[must_use]
    pub fn installed(&self) -> bool {
        find_program(self.name, &default_folders()).is_some()
    }

    /// How to install it on this system, if it runs here.
    #[must_use]
    pub fn install_here(&self) -> Option<&'static str> {
        if cfg!(target_os = "macos") {
            self.install[0]
        } else if cfg!(windows) {
            self.install[2]
        } else {
            self.install[1]
        }
    }

    /// Whether it's installed, or how to install it here, for people.
    #[must_use]
    pub fn state(&self) -> String {
        if self.installed() {
            "installed".to_owned()
        } else {
            self.install_here().map_or_else(
                || "not for this system".to_owned(),
                |i| format!("install: {i}"),
            )
        }
    }
}

/// The known picker the `picker` preference names, if it names one.
#[must_use]
pub fn known(preference: &str) -> Option<&'static Known> {
    KNOWN.iter().find(|k| k.name == preference.trim())
}

/// The picker in use, and why, for people: the `picker` preference (a known picker's name,
/// any command, or `builtin`), or else the first known picker installed, or else the
/// built-in browser.
#[must_use]
pub fn picker_in_use(preference: Option<&str>) -> String {
    match preference.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) if p.eq_ignore_ascii_case(BUILT_IN) => "the built-in browser (chosen)".into(),
        Some(p) => format!("{p} (chosen)"),
        None => KNOWN.iter().find(|k| k.installed()).map_or_else(
            || "the built-in browser (no picker installed)".into(),
            |k| format!("{} (found installed)", k.name),
        ),
    }
}

/// The editor for cards, and where it comes from: the `editor` preference, then `$VISUAL`,
/// then `$EDITOR`, then the system's default.
#[must_use]
pub fn editor_in_use(preference: Option<&str>) -> (String, &'static str) {
    if let Some(chosen) = preference.filter(|e| !e.trim().is_empty()) {
        return (chosen.to_owned(), "Carrel's editor setting");
    }
    for (var, from) in [("VISUAL", "$VISUAL"), ("EDITOR", "$EDITOR")] {
        if let Some(chosen) = std::env::var(var).ok().filter(|e| !e.trim().is_empty()) {
            return (chosen, from);
        }
    }
    (
        if cfg!(windows) { "notepad" } else { "vi" }.to_owned(),
        "the system's default",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chosen_picker_or_editor_is_used_and_says_so() {
        assert_eq!(picker_in_use(Some("lf")), "lf (chosen)");
        assert_eq!(
            picker_in_use(Some("Builtin")),
            "the built-in browser (chosen)"
        );
        assert!(picker_in_use(None).contains('('));
        assert_eq!(known("yazi").map(|k| k.name), Some("yazi"));
        assert_eq!(known("my-picker"), None);
        assert_eq!(
            editor_in_use(Some("hx")),
            ("hx".to_owned(), "Carrel's editor setting")
        );
    }
}
