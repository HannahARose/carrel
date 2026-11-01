//! Downloading and deleting models from the Settings screen, in either format, as
//! `carrel models download` and `carrel models remove` do (`docs/design/models.md`). Carrel
//! asks before either.

use carrel_core::models::{CATALOG, Model, mlx_dir};

use super::app::{App, Confirm};
use super::review::{Picker, Purpose};
use crate::models::Format;

/// Downloading or deleting one model in one format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChange {
    /// The catalog model's name.
    pub name: String,
    /// Its format: one GGUF file, or a folder for MLX.
    pub format: Format,
    /// Whether it's deleted rather than downloaded.
    pub delete: bool,
}

/// Whether MLX builds can run here: on Apple-silicon Macs.
const MLX_RUNS_HERE: bool = cfg!(all(target_os = "macos", target_arch = "aarch64"));

/// The format's name, as `--format` takes it.
fn format_name(format: Format) -> &'static str {
    match format {
        Format::Gguf => "gguf",
        Format::Mlx => "mlx",
    }
}

/// Where `model` is or goes in `format`, in the models folder `dir`, and its size.
fn place(
    model: &Model,
    format: Format,
    dir: &std::path::Path,
) -> Option<(std::path::PathBuf, u64)> {
    match format {
        Format::Gguf => Some((dir.join(model.file_name()), model.size)),
        Format::Mlx => model
            .mlx
            .map(|build| (mlx_dir(dir).join(model.name), build.size())),
    }
}

/// Every model change the Settings screen offers, by the words shown for it: downloading a
/// pinned catalog model in a format it comes in that isn't here yet, and deleting one that is.
#[must_use]
pub fn model_choices(dir: &std::path::Path) -> Vec<(String, ModelChange)> {
    let mut downloads = Vec::new();
    let mut deletes = Vec::new();
    for model in CATALOG
        .iter()
        .filter(|m| m.is_pinned() && m.comes_with.is_none())
    {
        for format in [Format::Gguf, Format::Mlx] {
            let Some((path, size)) = place(model, format, dir) else {
                continue;
            };
            let change = |delete| ModelChange {
                name: model.name.to_owned(),
                format,
                delete,
            };
            let shown = format!(
                "{} as {} ({})",
                model.name,
                format_name(format),
                crate::ingest::human_size(size)
            );
            if path.exists() {
                deletes.push((format!("Delete {shown}"), change(true)));
            } else if format == Format::Gguf || MLX_RUNS_HERE {
                downloads.push((format!("Download {shown}"), change(false)));
            }
        }
    }
    downloads.extend(deletes);
    downloads
}

/// The confirmation's title, question, detail and yes key, for the confirmation popup.
#[must_use]
pub fn model_change_confirm(
    change: &ModelChange,
    about: &str,
) -> (&'static str, String, String, &'static str) {
    let format = format_name(change.format);
    if change.delete {
        (
            " Delete model ",
            format!("Delete {} ({format})?", change.name),
            about.to_owned(),
            " delete   ",
        )
    } else {
        (
            " Download ",
            format!("Download {} ({format})?", change.name),
            about.to_owned(),
            " download   ",
        )
    }
}

/// The confirmation for downloading the catalog model `name` as GGUF, described by `about`.
#[must_use]
pub fn download_confirm(name: &str, about: &str) -> (&'static str, String, String, &'static str) {
    (
        " Download ",
        format!("Download {name}?"),
        about.to_owned(),
        " download   ",
    )
}

/// The work for a model change; it was asked about already.
pub fn model_change_work(change: ModelChange) -> (String, super::Work) {
    let verb = if change.delete {
        "Deleting"
    } else {
        "Downloading"
    };
    (
        format!("{verb} {} ({})…", change.name, format_name(change.format)),
        Box::new(move || {
            if change.delete {
                crate::models::remove(&change.name, change.format, true)
            } else {
                crate::models::download_quietly(&change.name, change.format)
            }
        }),
    )
}

impl App {
    /// Offers the models to download or delete, or says there are none.
    pub(super) fn models_picker(&mut self) {
        let Some(dir) = carrel_core::local::models_dir() else {
            self.message = Some("No models folder: set CARREL_MODELS_DIR".into());
            return;
        };
        let choices = model_choices(&dir);
        if choices.is_empty() {
            self.message = Some("No catalog model can be downloaded here".into());
            return;
        }
        self.picker = Some(Picker {
            collections: choices.iter().map(|(words, _)| words.clone()).collect(),
            filter: String::new(),
            chosen: 0,
            purpose: Purpose::Models(choices),
        });
    }

    /// Asks before the model change shown as `words` among `choices`.
    pub(super) fn confirm_model_change(&mut self, choices: &[(String, ModelChange)], words: &str) {
        let Some((_, change)) = choices.iter().find(|(w, _)| w == words) else {
            return;
        };
        let (Some(model), Some(dir)) = (
            carrel_core::models::find(&change.name),
            carrel_core::local::models_dir(),
        ) else {
            return;
        };
        let Some((path, size)) = place(model, change.format, &dir) else {
            return;
        };
        let about = if change.delete {
            format!(
                "{} ({}) is deleted from {}; download it again whenever you like.",
                model.title,
                crate::ingest::human_size(size),
                path.display()
            )
        } else {
            format!(
                "{} ({}, {} license), from huggingface.co to {}.",
                model.title,
                crate::ingest::human_size(size),
                model.license,
                path.display()
            )
        };
        self.confirm = Some(Confirm::ModelChange {
            change: change.clone(),
            about,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_here_can_be_deleted_and_the_rest_downloaded_in_each_format() {
        let dir = tempfile::tempdir().unwrap();
        let model = CATALOG
            .iter()
            .find(|m| m.is_pinned() && m.comes_with.is_none() && m.mlx.is_some())
            .expect("a model with an MLX build");
        let before = model_choices(dir.path());
        let gguf = format!("Download {} as gguf", model.name);
        assert!(
            before.iter().any(|(w, _)| w.starts_with(&gguf)),
            "{before:?}"
        );
        assert_eq!(
            before
                .iter()
                .any(|(w, _)| w.starts_with(&format!("Download {} as mlx", model.name))),
            MLX_RUNS_HERE
        );
        assert!(!before.iter().any(|(w, _)| w.starts_with("Delete")));

        std::fs::write(dir.path().join(model.file_name()), "weights").unwrap();
        std::fs::create_dir_all(mlx_dir(dir.path()).join(model.name)).unwrap();
        let after = model_choices(dir.path());
        for format in ["gguf", "mlx"] {
            let words = format!("Delete {} as {format}", model.name);
            let (_, change) = after
                .iter()
                .find(|(w, _)| w.starts_with(&words))
                .unwrap_or_else(|| panic!("{words} in {after:?}"));
            assert!(change.delete);
            assert_eq!(format_name(change.format), format);
        }
        assert!(!after.iter().any(|(w, _)| w.starts_with(&gguf)));
    }
}
