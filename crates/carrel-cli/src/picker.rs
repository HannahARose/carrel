//! File pickers for choosing what to ingest (`docs/design/tui-ingest.md`): the owner's own
//! terminal file manager, run the way the editor is, reading back the paths chosen.

use std::path::{Path, PathBuf};

use crate::Failure;

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
    /// What it's like, for the Settings screen.
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
        crate::paths::program(self.name).is_some()
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
}

/// How the owner picks files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Run this command's words.
    Command(Vec<String>),
    /// Carrel's own browser.
    BuiltIn,
}

/// The picker to use: the `picker` preference (a known picker's name, any command, or
/// `builtin`), or else the first known picker installed, or else the built-in browser.
#[must_use]
pub fn choose(preference: Option<&str>) -> Choice {
    if let Some(wanted) = preference.map(str::trim).filter(|p| !p.is_empty()) {
        if wanted.eq_ignore_ascii_case(BUILT_IN) {
            return Choice::BuiltIn;
        }
        let command = KNOWN
            .iter()
            .find(|k| k.name == wanted)
            .map_or(wanted, |k| k.command);
        return Choice::Command(crate::paths::split_words(command));
    }
    KNOWN
        .iter()
        .find(|k| k.installed())
        .map_or(Choice::BuiltIn, |k| {
            Choice::Command(crate::paths::split_words(k.command))
        })
}

/// Runs the picker `words` in `folder`, with the terminal to itself, and gives the paths
/// chosen, absolute. An empty list means nothing was chosen.
///
/// # Errors
///
/// [`Failure`] if the picker can't be started, or exits with an error.
pub fn run(words: &[String], folder: &Path) -> Result<Vec<PathBuf>, Failure> {
    let (program, args) = words
        .split_first()
        .ok_or_else(|| Failure::new("the picker command is empty"))?;
    let out = std::env::temp_dir().join(format!("carrel-picked-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let to_file = args.iter().any(|a| a.contains("{out}"));
    let args: Vec<String> = args
        .iter()
        .map(|a| a.replace("{out}", &out.to_string_lossy()))
        .collect();
    let mut command = std::process::Command::new(program);
    command.args(&args).current_dir(folder);
    let text = if to_file {
        let status = command
            .status()
            .map_err(|e| Failure::new(format!("couldn't start the picker {program}: {e}")))?;
        let text = std::fs::read_to_string(&out).unwrap_or_default();
        let _ = std::fs::remove_file(&out);
        if !status.success() && text.trim().is_empty() {
            return Ok(Vec::new());
        }
        text
    } else {
        let output = command
            .stdin(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .output()
            .map_err(|e| Failure::new(format!("couldn't start the picker {program}: {e}")))?;
        // fzf exits with 130 when cancelled, and 1 when nothing matched: nothing chosen.
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            let path = PathBuf::from(l);
            if path.is_absolute() {
                path
            } else {
                folder.join(path)
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preference_names_a_known_picker_a_command_or_the_built_in() {
        assert_eq!(
            choose(Some("lf")),
            Choice::Command(vec!["lf".into(), "-selection-path={out}".into()])
        );
        assert_eq!(
            choose(Some("my-picker --out {out}")),
            Choice::Command(vec!["my-picker".into(), "--out".into(), "{out}".into()])
        );
        assert_eq!(choose(Some("builtin")), Choice::BuiltIn);
    }

    #[cfg(unix)]
    #[test]
    fn a_picker_writes_paths_to_a_file_or_standard_output() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        let to_file: Vec<String> = [
            "sh",
            "-c",
            "printf 'a.txt\\n/abs/b.pdf\\n' > \"$0\"",
            "{out}",
        ]
        .map(str::to_owned)
        .to_vec();
        assert_eq!(
            run(&to_file, dir.path()).unwrap(),
            [dir.path().join("a.txt"), PathBuf::from("/abs/b.pdf")]
        );
        let to_stdout: Vec<String> = ["sh", "-c", "echo a.txt"].map(str::to_owned).to_vec();
        assert_eq!(
            run(&to_stdout, dir.path()).unwrap(),
            [dir.path().join("a.txt")]
        );
        let cancelled: Vec<String> = ["sh", "-c", "exit 1", "{out}"].map(str::to_owned).to_vec();
        assert_eq!(run(&cancelled, dir.path()).unwrap(), Vec::<PathBuf>::new());
    }
}
