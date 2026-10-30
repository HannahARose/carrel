//! Models Carrel can download for local providers (`docs/design/models.md`).
//!
//! [`CATALOG`] lists the models Carrel knows work, each pinned to one file at a fixed commit
//! with its size and SHA-256. Downloading goes through the network helper, `carrel-fetch`,
//! the only Carrel program that goes online: [`download`] hands it a [`FetchRequest`], reads
//! its [`FetchEvent`]s, then checks the file itself before giving it the name the llama.cpp
//! provider looks for.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// A model in the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Model {
    /// Its short name, used in commands and as its file's name.
    pub name: &'static str,
    /// What it is, in words.
    pub title: &'static str,
    /// The Hugging Face repository holding it.
    pub repository: &'static str,
    /// The repository's commit the file is pinned to.
    pub revision: &'static str,
    /// The file's name in the repository.
    pub file: &'static str,
    /// The file's size in bytes.
    pub size: u64,
    /// The file's SHA-256, in lowercase hexadecimal.
    pub sha256: &'static str,
    /// Its license's SPDX identifier.
    pub license: &'static str,
    /// The memory a machine should have to run it, in gigabytes.
    pub memory_gb: u8,
    /// Whether the benchmark (#33) recommends it for machines with that much memory.
    pub recommended: bool,
}

impl Model {
    /// Where the file is downloaded from.
    #[must_use]
    pub fn url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repository, self.revision, self.file
        )
    }

    /// The name it's saved under in the models folder.
    #[must_use]
    pub fn file_name(&self) -> String {
        format!("{}.gguf", self.name)
    }
}

/// The models Carrel can download. Each is open weights under a permissive license, not gated
/// behind an account, and loadable by the llama.cpp Carrel ships.
pub const CATALOG: &[Model] = &[
    Model {
        name: "granite-4.2-3b",
        title: "IBM Granite 4.2 3B, Q4_K_M",
        repository: "ibm-granite/granite-4.2-3b-GGUF",
        revision: "c40945d71cd90f249a56985e8155551a9188dc30",
        file: "granite-4.2-3b-Q4_K_M.gguf",
        size: 2_244_011_552,
        sha256: "e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5",
        license: "Apache-2.0",
        memory_gb: 8,
        recommended: false,
    },
    Model {
        name: "qwen3.5-4b",
        title: "Qwen3.5 4B, Q4_K_M",
        repository: "unsloth/Qwen3.5-4B-GGUF",
        revision: "e87f176479d0855a907a41277aca2f8ee7a09523",
        file: "Qwen3.5-4B-Q4_K_M.gguf",
        size: 2_740_937_888,
        sha256: "00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4",
        license: "Apache-2.0",
        memory_gb: 8,
        recommended: false,
    },
    Model {
        name: "gemma-4-e4b",
        title: "Gemma 4 E4B, Google's QAT Q4_0",
        repository: "google/gemma-4-E4B-it-qat-q4_0-gguf",
        revision: "4b4a2c1d584be7264f87aac328a1bc739ce81b6c",
        file: "gemma-4-E4B_q4_0-it.gguf",
        size: 5_154_941_280,
        sha256: "676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee",
        license: "Apache-2.0",
        memory_gb: 16,
        recommended: false,
    },
    Model {
        name: "granite-4.2-8b",
        title: "IBM Granite 4.2 8B, Q4_K_M",
        repository: "ibm-granite/granite-4.2-8b-GGUF",
        revision: "93f3f6a8938ee922b784cf4e5b4203cd3428df8f",
        file: "granite-4.2-8b-Q4_K_M.gguf",
        size: 5_347_917_952,
        sha256: "16a9369d0805f80b7377d25d87f937a90c05dc04ad79173a52001e42c9aab311",
        license: "Apache-2.0",
        memory_gb: 16,
        recommended: false,
    },
    Model {
        name: "qwen3.5-9b",
        title: "Qwen3.5 9B, Q4_K_M",
        repository: "unsloth/Qwen3.5-9B-GGUF",
        revision: "3885219b6810b007914f3a7950a8d1b469d598a5",
        file: "Qwen3.5-9B-Q4_K_M.gguf",
        size: 5_680_522_464,
        sha256: "03b74727a860a56338e042c4420bb3f04b2fec5734175f4cb9fa853daf52b7e8",
        license: "Apache-2.0",
        memory_gb: 16,
        recommended: false,
    },
];

/// The hosts a catalog download may reach: Hugging Face, and its CDN, which it redirects to.
pub const ALLOWED_HOSTS: &[&str] = &["huggingface.co", ".hf.co"];

/// The catalog's model called `name`.
#[must_use]
pub fn find(name: &str) -> Option<&'static Model> {
    CATALOG.iter().find(|m| m.name == name)
}

/// What the network helper is asked to do, as one JSON value on its standard input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchRequest {
    /// The `https` URL to download.
    pub url: String,
    /// The hosts it and any redirect may be on. One starting with `.` allows any host ending
    /// with it.
    pub hosts: Vec<String>,
    /// The size the file must have, in bytes.
    pub size: u64,
    /// The SHA-256 it must have, in lowercase hexadecimal.
    pub sha256: String,
    /// The file to write. If it exists, the download continues from its end.
    pub to: PathBuf,
    /// The log to append a line to for every request made, if any.
    pub log: Option<PathBuf>,
}

/// What the network helper reports, one JSON value per line on its standard output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum FetchEvent {
    /// How much of the file is written.
    Progress {
        /// Bytes written so far, including any continued from an earlier attempt.
        done: u64,
        /// The file's full size.
        total: u64,
    },
    /// The file is complete, with the size and SHA-256 asked for.
    Done {
        /// Its size.
        bytes: u64,
        /// Its SHA-256.
        sha256: String,
    },
    /// The download stopped. A partial file is kept so it can continue, unless it was wrong.
    Failed {
        /// Why, in words.
        message: String,
    },
}

/// Whether `host` is allowed by `hosts`, as [`FetchRequest::hosts`] describes.
#[must_use]
pub fn host_allowed(host: &str, hosts: &[String]) -> bool {
    let host = host.to_ascii_lowercase();
    hosts.iter().any(|allowed| {
        let allowed = allowed.to_ascii_lowercase();
        match allowed.strip_prefix('.') {
            Some(domain) => host.ends_with(&allowed) && host.len() > domain.len() + 1,
            None => host == allowed,
        }
    })
}

/// Why a model couldn't be downloaded or removed.
#[derive(Debug)]
pub enum ModelError {
    /// No model in the catalog has this name.
    Unknown(String),
    /// There's no models folder: no configuration folder and no `CARREL_MODELS_DIR`.
    NoFolder,
    /// The `carrel-fetch` helper isn't installed.
    NoHelper,
    /// The helper reported a failure, or stopped without finishing.
    Fetch(String),
    /// The finished file didn't have the size or SHA-256 the catalog gives.
    Mismatch {
        /// What was expected.
        expected: String,
        /// What the file had.
        found: String,
    },
    /// The model isn't downloaded.
    NotDownloaded(String),
    /// Reading or writing the models folder failed.
    Io(std::io::Error),
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(name) => {
                write!(f, "no model called {name}; `carrel models` lists them")
            }
            Self::NoFolder => write!(f, "no models folder: set CARREL_MODELS_DIR"),
            Self::NoHelper => write!(
                f,
                "the network helper, carrel-fetch, isn't installed beside carrel or on PATH"
            ),
            Self::Fetch(message) => write!(f, "download failed: {message}"),
            Self::Mismatch { expected, found } => write!(
                f,
                "the downloaded file isn't the one expected ({expected}, found {found}), so it was deleted"
            ),
            Self::NotDownloaded(name) => write!(f, "{name} isn't downloaded"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ModelError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ModelError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Something that downloads a file as a [`FetchRequest`] asks: the network helper, or a test
/// double.
pub trait Fetcher {
    /// Downloads, calling `progress` with each [`FetchEvent::Progress`].
    ///
    /// # Errors
    ///
    /// [`ModelError::Fetch`] with the helper's message.
    fn fetch(
        &self,
        request: &FetchRequest,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<(), ModelError>;
}

/// The network helper, `carrel-fetch`.
#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
#[derive(Debug, Clone)]
pub struct Helper {
    program: PathBuf,
}

#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
impl Helper {
    /// The helper beside `carrel` or on `PATH`.
    ///
    /// # Errors
    ///
    /// [`ModelError::NoHelper`] if it isn't found.
    pub fn find() -> Result<Self, ModelError> {
        crate::providers::process::find_program(
            "carrel-fetch",
            &crate::providers::process::default_folders(),
        )
        .map(Self::at)
        .ok_or(ModelError::NoHelper)
    }

    /// The helper at `program`.
    #[must_use]
    pub fn at(program: PathBuf) -> Self {
        Self { program }
    }
}

#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
impl Fetcher for Helper {
    fn fetch(
        &self,
        request: &FetchRequest,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<(), ModelError> {
        use std::io::{BufRead as _, Write as _};
        use std::process::{Command, Stdio};

        let mut child = Command::new(&self.program)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            let line = serde_json::to_string(request).map_err(std::io::Error::other)?;
            writeln!(stdin, "{line}")?;
        }
        let mut outcome = Err(ModelError::Fetch(
            "the helper stopped without finishing".into(),
        ));
        if let Some(stdout) = child.stdout.take() {
            for line in std::io::BufReader::new(stdout).lines() {
                match serde_json::from_str(&line?) {
                    Ok(FetchEvent::Progress { done, total }) => progress(done, total),
                    Ok(FetchEvent::Done { .. }) => outcome = Ok(()),
                    Ok(FetchEvent::Failed { message }) => outcome = Err(ModelError::Fetch(message)),
                    Err(e) => outcome = Err(ModelError::Fetch(format!("unreadable report: {e}"))),
                }
            }
        }
        child.wait()?;
        outcome
    }
}

/// A file in the models folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Installed {
    /// Its name without `.gguf`, which the llama.cpp provider offers it as.
    pub name: String,
    /// Where it is.
    pub path: PathBuf,
    /// Its size in bytes.
    pub size: u64,
}

/// The `.gguf` files in `dir`, by name, whether from the catalog or added by hand. A missing
/// folder has none.
///
/// # Errors
///
/// [`std::io::Error`] if the folder can't be read.
pub fn installed(dir: &Path) -> std::io::Result<Vec<Installed>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
        {
            continue;
        }
        let metadata = entry.metadata()?;
        if !metadata.is_file() {
            continue;
        }
        if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
            found.push(Installed {
                name: name.to_owned(),
                path: path.clone(),
                size: metadata.len(),
            });
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// Where a model's partial download is kept.
#[must_use]
pub fn part_path(model: &Model, dir: &Path) -> PathBuf {
    dir.join(format!("{}.part", model.file_name()))
}

/// The device log every network request is recorded in.
#[must_use]
pub fn network_log() -> Option<PathBuf> {
    crate::local::config_dir().map(|dir| dir.join("logs").join("network.log"))
}

/// Downloads `model` into `dir` through `fetcher`, continuing a partial download if there is
/// one. The file is renamed to [`Model::file_name`] only once its size and SHA-256 match the
/// catalog's; if they don't, it's deleted.
///
/// # Errors
///
/// [`ModelError::Fetch`] if the download stops, [`ModelError::Mismatch`] if the file isn't
/// the one expected, or [`ModelError::Io`].
pub fn download(
    model: &Model,
    dir: &Path,
    fetcher: &dyn Fetcher,
    log: Option<&Path>,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, ModelError> {
    std::fs::create_dir_all(dir)?;
    let part = part_path(model, dir);
    let request = FetchRequest {
        url: model.url(),
        hosts: ALLOWED_HOSTS.iter().map(|h| (*h).to_owned()).collect(),
        size: model.size,
        sha256: model.sha256.to_owned(),
        to: part.clone(),
        log: log.map(Path::to_owned),
    };
    fetcher.fetch(&request, progress)?;
    // The helper checked the file too, but it's a separate program: check it here as well.
    let (size, sha256) = hash_file(&part)?;
    if size != model.size || sha256 != model.sha256 {
        std::fs::remove_file(&part)?;
        return Err(ModelError::Mismatch {
            expected: format!("{} bytes, SHA-256 {}", model.size, model.sha256),
            found: format!("{size} bytes, SHA-256 {sha256}"),
        });
    }
    let path = dir.join(model.file_name());
    std::fs::rename(&part, &path)?;
    Ok(path)
}

/// Deletes the downloaded model called `name` from `dir`, and any partial download of it.
///
/// # Errors
///
/// [`ModelError::NotDownloaded`] if there's nothing to delete, or [`ModelError::Io`].
pub fn remove(name: &str, dir: &Path) -> Result<PathBuf, ModelError> {
    // Only a file directly in the folder: a name like `../x` mustn't reach outside it.
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return Err(ModelError::NotDownloaded(name.to_owned()));
    }
    let path = dir.join(format!("{name}.gguf"));
    let part = dir.join(format!("{name}.gguf.part"));
    let had_part = part.is_file();
    if had_part {
        std::fs::remove_file(&part)?;
    }
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && had_part => Ok(part),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(ModelError::NotDownloaded(name.to_owned()))
        }
        Err(e) => Err(e.into()),
    }
}

/// A file's size and SHA-256, in lowercase hexadecimal.
///
/// # Errors
///
/// [`std::io::Error`] if it can't be read.
pub fn hash_file(path: &Path) -> std::io::Result<(u64, String)> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    let mut size = 0;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    Ok((size, hex(&hasher.finalize())))
}

/// Bytes as lowercase hexadecimal.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes `contents` to the request's file, as a download would.
    struct Writes(Vec<u8>);

    impl Fetcher for Writes {
        fn fetch(
            &self,
            request: &FetchRequest,
            progress: &mut dyn FnMut(u64, u64),
        ) -> Result<(), ModelError> {
            std::fs::write(&request.to, &self.0)?;
            progress(self.0.len() as u64, request.size);
            Ok(())
        }
    }

    struct Fails;

    impl Fetcher for Fails {
        fn fetch(&self, _: &FetchRequest, _: &mut dyn FnMut(u64, u64)) -> Result<(), ModelError> {
            Err(ModelError::Fetch("connection reset".into()))
        }
    }

    fn model_of(contents: &[u8]) -> (Model, String) {
        let sha256 = hex(&Sha256::digest(contents));
        let model = Model {
            sha256: "",
            size: contents.len() as u64,
            ..CATALOG[0]
        };
        (model, sha256)
    }

    #[test]
    fn the_catalog_is_pinned_and_permissive() {
        for model in CATALOG {
            assert_eq!(model.revision.len(), 40, "{}", model.name);
            assert_eq!(model.sha256.len(), 64, "{}", model.name);
            assert!(
                model
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            );
            assert!(
                ["Apache-2.0", "MIT"].contains(&model.license),
                "{}",
                model.name
            );
            assert!(
                Path::new(model.file)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
            );
            assert!(model.url().starts_with("https://huggingface.co/"));
            assert!(crate::names::is_portable(&model.file_name()));
        }
        let mut names: Vec<_> = CATALOG.iter().map(|m| m.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), CATALOG.len());
    }

    #[test]
    fn hosts_match_exactly_or_by_domain() {
        let hosts: Vec<String> = ALLOWED_HOSTS.iter().map(|h| (*h).to_owned()).collect();
        assert!(host_allowed("huggingface.co", &hosts));
        assert!(host_allowed("HuggingFace.co", &hosts));
        assert!(host_allowed("us.aws.cdn.hf.co", &hosts));
        assert!(!host_allowed("hf.co.evil.example", &hosts));
        assert!(!host_allowed("evilhf.co", &hosts));
        assert!(!host_allowed("huggingface.co.evil.example", &hosts));
        assert!(!host_allowed("example.com", &hosts));
    }

    #[test]
    fn a_checked_download_gets_its_name() {
        let dir = tempfile::tempdir().unwrap();
        let contents = b"GGUF model".to_vec();
        let (mut model, sha256) = model_of(&contents);
        let sha256: &'static str = Box::leak(sha256.into_boxed_str());
        model.sha256 = sha256;
        let mut seen = Vec::new();
        let path = download(&model, dir.path(), &Writes(contents), None, &mut |d, t| {
            seen.push((d, t));
        })
        .unwrap();
        assert_eq!(path, dir.path().join("granite-4.2-3b.gguf"));
        assert!(!part_path(&model, dir.path()).exists());
        assert_eq!(seen, [(10, 10)]);
        let found = installed(dir.path()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "granite-4.2-3b");
    }

    #[test]
    fn a_file_that_does_not_match_is_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let (mut model, _) = model_of(b"GGUF model");
        model.sha256 = "0000000000000000000000000000000000000000000000000000000000000000";
        let error = download(
            &model,
            dir.path(),
            &Writes(b"GGUF model".to_vec()),
            None,
            &mut |_, _| {},
        )
        .unwrap_err();
        assert!(matches!(error, ModelError::Mismatch { .. }), "{error}");
        assert_eq!(installed(dir.path()).unwrap(), []);
        assert!(!part_path(&model, dir.path()).exists());
    }

    #[test]
    fn a_failed_download_names_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let error = download(&CATALOG[0], dir.path(), &Fails, None, &mut |_, _| {}).unwrap_err();
        assert!(error.to_string().contains("connection reset"));
        assert_eq!(installed(dir.path()).unwrap(), []);
    }

    #[test]
    fn removing_deletes_the_model_and_any_part() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.gguf"), b"x").unwrap();
        std::fs::write(dir.path().join("b.gguf.part"), b"x").unwrap();
        assert_eq!(remove("a", dir.path()).unwrap(), dir.path().join("a.gguf"));
        assert_eq!(
            remove("b", dir.path()).unwrap(),
            dir.path().join("b.gguf.part")
        );
        assert!(matches!(
            remove("c", dir.path()),
            Err(ModelError::NotDownloaded(_))
        ));
        assert_eq!(installed(dir.path()).unwrap(), []);
    }

    #[test]
    fn requests_and_events_round_trip_as_json() {
        let request = FetchRequest {
            url: "https://huggingface.co/x".into(),
            hosts: vec!["huggingface.co".into()],
            size: 3,
            sha256: "ab".into(),
            to: PathBuf::from("m.gguf.part"),
            log: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<FetchRequest>(&json).unwrap(),
            request
        );
        let event = FetchEvent::Progress { done: 1, total: 3 };
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(json, r#"{"event":"progress","done":1,"total":3}"#);
        assert_eq!(serde_json::from_str::<FetchEvent>(&json).unwrap(), event);
    }
}
