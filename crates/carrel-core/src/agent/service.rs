//! Starting the agent at log-in, as the operating system's service manager does it: a
//! launchd user agent on macOS, a systemd user service on Linux, and a Task Scheduler task at
//! log-on on Windows. What `carrel agent install|uninstall|status` does.
//!
//! Each writes only its own file and makes only its own entry, and uninstalling removes
//! exactly those:
//!
//! - macOS: `~/Library/LaunchAgents/io.github.hannaharose.carrel.agent.plist`, loaded with
//!   `launchctl bootstrap`.
//! - Linux: `~/.config/systemd/user/carrel-agent.service` (or under `$XDG_CONFIG_HOME`),
//!   enabled with `systemctl --user enable --now`, which links it into `default.target.wants`.
//! - Windows: the task "Carrel Agent", created with `schtasks /Create` from
//!   `carrel-agent.xml` in Carrel's configuration folder, which is kept to show what was
//!   installed.
//!
//! The definitions are plain text made here, so they're tested on any platform; running the
//! service manager goes through [`Manager`], which tests replace.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The launchd label, after the Mac app's bundle identifier.
pub const LAUNCHD_LABEL: &str = "io.github.hannaharose.carrel.agent";
/// The systemd user unit.
pub const SYSTEMD_UNIT: &str = "carrel-agent.service";
/// The Task Scheduler task.
pub const TASK_NAME: &str = "Carrel Agent";
/// The task's definition, kept in Carrel's configuration folder.
pub const TASK_FILE: &str = "carrel-agent.xml";

/// The flag that has the agent look for changes instead of hearing file events.
pub const POLL_FLAG: &str = "--poll";

/// A service manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// macOS: a launchd user agent.
    Launchd,
    /// Linux: a systemd user service.
    Systemd,
    /// Windows: a Task Scheduler task at log-on.
    TaskScheduler,
}

impl Platform {
    /// This platform's service manager, if Carrel knows one.
    #[must_use]
    pub fn current() -> Option<Self> {
        if cfg!(target_os = "macos") {
            Some(Self::Launchd)
        } else if cfg!(windows) {
            Some(Self::TaskScheduler)
        } else if cfg!(target_os = "linux") {
            Some(Self::Systemd)
        } else {
            None
        }
    }

    /// What the agent is installed as, for people.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Launchd => "a launchd user agent",
            Self::Systemd => "a systemd user service",
            Self::TaskScheduler => "a Task Scheduler task at log-on",
        }
    }
}

/// The folders a service definition goes in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Places {
    /// The person's home folder.
    pub home: PathBuf,
    /// `$XDG_CONFIG_HOME`, where systemd looks for user units, if set.
    pub xdg_config_home: Option<PathBuf>,
    /// Carrel's configuration folder, for the Windows task's definition and the log.
    pub config: PathBuf,
}

impl Places {
    /// This device's folders, from the environment: `None` without a home or configuration
    /// folder.
    #[must_use]
    pub fn here() -> Option<Self> {
        Some(Self {
            home: crate::local::home_dir()?,
            xdg_config_home: std::env::var_os("XDG_CONFIG_HOME")
                .filter(|d| !d.is_empty())
                .map(PathBuf::from),
            config: crate::local::config_dir()?,
        })
    }

    /// The file `platform`'s definition is written to.
    #[must_use]
    pub fn file(&self, platform: Platform) -> PathBuf {
        match platform {
            Platform::Launchd => self
                .home
                .join("Library/LaunchAgents")
                .join(format!("{LAUNCHD_LABEL}.plist")),
            Platform::Systemd => self
                .xdg_config_home
                .clone()
                .unwrap_or_else(|| self.home.join(".config"))
                .join("systemd/user")
                .join(SYSTEMD_UNIT),
            Platform::TaskScheduler => self.config.join(TASK_FILE),
        }
    }

    /// Where the agent's output goes when the service manager doesn't keep it itself.
    #[must_use]
    pub fn log(&self) -> PathBuf {
        self.config.join("workers").join("agent.log")
    }
}

/// What starts the agent: the program, the archives it follows, and the environment it needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    /// The `carrel` program.
    pub program: PathBuf,
    /// The archives' root folders.
    pub archives: Vec<PathBuf>,
    /// Whether the agent looks for changes every few seconds instead of through file
    /// events (`--poll`), as for archives on network drives.
    pub poll: bool,
    /// Environment variables to set, such as `PATH` (so Kopia and the providers are found,
    /// as in the terminal it was installed from) and `CARREL_CONFIG_DIR`.
    pub environment: Vec<(String, String)>,
    /// Where its output goes, for launchd and the Windows task.
    pub log: PathBuf,
}

impl Definition {
    /// The arguments after the program: `agent`, `--poll` if asked, and the archives.
    fn arguments(&self) -> Vec<String> {
        std::iter::once("agent".to_owned())
            .chain(self.poll.then(|| POLL_FLAG.to_owned()))
            .chain(self.archives.iter().map(|a| a.display().to_string()))
            .collect()
    }

    /// The definition's text for `platform`.
    #[must_use]
    pub fn contents(&self, platform: Platform) -> String {
        match platform {
            Platform::Launchd => self.plist(),
            Platform::Systemd => self.unit(),
            Platform::TaskScheduler => self.task(),
        }
    }

    fn plist(&self) -> String {
        let mut text = String::from(concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ",
            "\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
            "<plist version=\"1.0\">\n<dict>\n",
        ));
        let string = |s: &str| format!("<string>{}</string>", xml(s));
        let _ = write!(text, "  <key>Label</key>\n  {}\n", string(LAUNCHD_LABEL));
        text += "  <key>ProgramArguments</key>\n  <array>\n";
        for arg in std::iter::once(self.program.display().to_string()).chain(self.arguments()) {
            let _ = writeln!(text, "    {}", string(&arg));
        }
        text += "  </array>\n";
        if !self.environment.is_empty() {
            text += "  <key>EnvironmentVariables</key>\n  <dict>\n";
            for (key, value) in &self.environment {
                let _ = write!(text, "    <key>{}</key>\n    {}\n", xml(key), string(value));
            }
            text += "  </dict>\n";
        }
        let log = string(&self.log.display().to_string());
        let _ = write!(
            text,
            concat!(
                "  <key>RunAtLoad</key>\n  <true/>\n",
                "  <key>KeepAlive</key>\n",
                "  <dict>\n    <key>SuccessfulExit</key>\n    <false/>\n  </dict>\n",
                "  <key>ProcessType</key>\n  <string>Background</string>\n",
                "  <key>StandardOutPath</key>\n  {log}\n",
                "  <key>StandardErrorPath</key>\n  {log}\n",
                "</dict>\n</plist>\n",
            ),
            log = log
        );
        text
    }

    fn unit(&self) -> String {
        let exec: Vec<String> = std::iter::once(self.program.display().to_string())
            .chain(self.arguments())
            .map(|a| systemd_quote(&a))
            .collect();
        let mut text = String::from(concat!(
            "[Unit]\n",
            "Description=Carrel agent: drafts the queue, keeps the index current, ",
            "notices work sessions and takes scheduled snapshots\n",
            "Documentation=https://github.com/HannahARose/carrel/blob/dev/docs/guide/user-guide.md\n",
            "\n[Service]\n",
            "Type=simple\n",
        ));
        let _ = writeln!(text, "ExecStart={}", exec.join(" "));
        for (key, value) in &self.environment {
            let _ = writeln!(
                text,
                "Environment={}",
                systemd_quote(&format!("{key}={value}"))
            );
        }
        text += concat!(
            "Restart=on-failure\n",
            "RestartSec=30\n",
            "Nice=10\n",
            "\n[Install]\n",
            "WantedBy=default.target\n",
        );
        text
    }

    /// The command the Windows task runs: PowerShell, hidden, so no console window stays
    /// open, with the agent's output appended to its log.
    fn powershell_command(&self) -> String {
        let mut words: Vec<String> = vec![
            "&".into(),
            powershell_quote(&self.program.display().to_string()),
        ];
        words.push("agent".into());
        words.extend(self.poll.then(|| POLL_FLAG.to_owned()));
        words.extend(
            self.archives
                .iter()
                .map(|a| powershell_quote(&a.display().to_string())),
        );
        words.push("*>>".into());
        words.push(powershell_quote(&self.log.display().to_string()));
        let mut command = String::new();
        for (key, value) in &self.environment {
            let _ = write!(command, "$env:{key} = {}; ", powershell_quote(value));
        }
        command + &words.join(" ")
    }

    fn task(&self) -> String {
        let arguments = format!(
            "-NoProfile -NonInteractive -WindowStyle Hidden -Command \"{}\"",
            self.powershell_command()
        );
        format!(
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n",
                "<Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n",
                "  <RegistrationInfo>\n",
                "    <Description>Carrel agent: drafts the queue, keeps the index current, ",
                "notices work sessions and takes scheduled snapshots.</Description>\n",
                "  </RegistrationInfo>\n",
                "  <Triggers>\n    <LogonTrigger>\n      <Enabled>true</Enabled>\n",
                "    </LogonTrigger>\n  </Triggers>\n",
                "  <Principals>\n    <Principal id=\"Author\">\n",
                "      <LogonType>InteractiveToken</LogonType>\n",
                "      <RunLevel>LeastPrivilege</RunLevel>\n",
                "    </Principal>\n  </Principals>\n",
                "  <Settings>\n",
                "    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\n",
                "    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\n",
                "    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\n",
                "    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>\n",
                "    <Priority>7</Priority>\n",
                "    <RestartOnFailure>\n      <Interval>PT1M</Interval>\n",
                "      <Count>3</Count>\n    </RestartOnFailure>\n",
                "  </Settings>\n",
                "  <Actions Context=\"Author\">\n    <Exec>\n",
                "      <Command>powershell.exe</Command>\n",
                "      <Arguments>{}</Arguments>\n",
                "    </Exec>\n  </Actions>\n",
                "</Task>\n",
            ),
            xml(&arguments)
        )
    }
}

/// `text` escaped for XML.
fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn unxml(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

/// `word` quoted for a systemd unit's command line, where `%` and `$` are special too.
fn systemd_quote(word: &str) -> String {
    let escaped = word
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%")
        .replace('$', "$$");
    format!("\"{escaped}\"")
}

/// The words of a systemd command line written by [`systemd_quote`].
fn systemd_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut word = String::new();
        while let Some(c) = chars.next() {
            match c {
                '\\' => word.extend(chars.next()),
                '%' | '$' if chars.peek() == Some(&c) => {
                    chars.next();
                    word.push(c);
                }
                '"' => break,
                c => word.push(c),
            }
        }
        words.push(word);
    }
    words
}

/// `word` as a PowerShell string: single quotes, with a single quote doubled.
fn powershell_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "''"))
}

/// The single-quoted PowerShell strings in `text`, in order.
fn powershell_strings(text: &str) -> Vec<String> {
    let mut strings = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\'' {
            continue;
        }
        let mut word = String::new();
        while let Some(c) = chars.next() {
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                    word.push('\'');
                } else {
                    break;
                }
            } else {
                word.push(c);
            }
        }
        strings.push(word);
    }
    strings
}

/// The archives a definition Carrel wrote for `platform` follows, read back from its text.
#[must_use]
pub fn archives_in(platform: Platform, contents: &str) -> Vec<PathBuf> {
    let after_agent = |words: Vec<String>| -> Vec<PathBuf> {
        words
            .into_iter()
            .skip_while(|w| w != "agent")
            .skip(1)
            .filter(|w| w != POLL_FLAG)
            .map(PathBuf::from)
            .collect()
    };
    match platform {
        Platform::Launchd => {
            let Some(start) = contents.find("<key>ProgramArguments</key>") else {
                return Vec::new();
            };
            let rest = &contents[start..];
            let end = rest.find("</array>").unwrap_or(rest.len());
            let words = rest[..end]
                .lines()
                .filter_map(|l| {
                    let l = l.trim();
                    l.strip_prefix("<string>")?.strip_suffix("</string>")
                })
                .map(unxml)
                .collect();
            after_agent(words)
        }
        Platform::Systemd => contents
            .lines()
            .find_map(|l| l.strip_prefix("ExecStart="))
            .map(|l| after_agent(systemd_words(l)))
            .unwrap_or_default(),
        Platform::TaskScheduler => {
            let Some(arguments) = contents
                .split_once("<Arguments>")
                .and_then(|(_, rest)| rest.split_once("</Arguments>"))
                .map(|(arguments, _)| unxml(arguments))
            else {
                return Vec::new();
            };
            // After the environment's assignments: the program, the archives, then the log.
            let command = arguments
                .rsplit_once("; &")
                .map_or(arguments.as_str(), |(_, c)| c);
            let mut strings = powershell_strings(command);
            strings.pop();
            strings.into_iter().skip(1).map(PathBuf::from).collect()
        }
    }
}

/// Runs the service manager's commands.
pub trait Manager {
    /// Runs `program` with `args`.
    ///
    /// # Errors
    ///
    /// What went wrong, in words, if it couldn't be run or failed.
    fn run(&mut self, program: &str, args: &[String]) -> Result<(), String>;
}

/// The service manager itself.
#[derive(Debug, Default)]
pub struct System;

impl Manager for System {
    fn run(&mut self, program: &str, args: &[String]) -> Result<(), String> {
        let out = std::process::Command::new(program)
            .args(args)
            .output()
            .map_err(|e| format!("can't run {program}: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            let said = String::from_utf8_lossy(&out.stderr);
            let said = said.trim();
            Err(if said.is_empty() {
                format!("{program} failed ({})", out.status)
            } else {
                format!("{program}: {said}")
            })
        }
    }
}

/// A command, as a line for people.
fn command_line(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_owned())
        .chain(args.iter().map(|a| {
            if a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        }))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The commands that start the service from its file at `file`, and that stop it and
/// forget it, for `platform`. `uid` is the person's user id, for launchd.
fn commands(platform: Platform, file: &Path, uid: Option<u32>) -> (Commands, Commands) {
    let s = |words: &[&str]| -> Vec<String> { words.iter().map(|w| (*w).to_owned()).collect() };
    let file = file.display().to_string();
    match platform {
        Platform::Launchd => {
            let domain = format!("gui/{}", uid.unwrap_or_default());
            (
                vec![("launchctl", vec!["bootstrap".into(), domain.clone(), file])],
                vec![(
                    "launchctl",
                    vec!["bootout".into(), format!("{domain}/{LAUNCHD_LABEL}")],
                )],
            )
        }
        Platform::Systemd => (
            vec![
                ("systemctl", s(&["--user", "daemon-reload"])),
                ("systemctl", s(&["--user", "enable", "--now", SYSTEMD_UNIT])),
            ],
            vec![(
                "systemctl",
                s(&["--user", "disable", "--now", SYSTEMD_UNIT]),
            )],
        ),
        Platform::TaskScheduler => (
            vec![
                (
                    "schtasks",
                    vec![
                        "/Create".into(),
                        "/TN".into(),
                        TASK_NAME.into(),
                        "/XML".into(),
                        file,
                        "/F".into(),
                    ],
                ),
                ("schtasks", s(&["/Run", "/TN", TASK_NAME])),
            ],
            vec![
                ("schtasks", s(&["/End", "/TN", TASK_NAME])),
                ("schtasks", s(&["/Delete", "/TN", TASK_NAME, "/F"])),
            ],
        ),
    }
}

type Commands = Vec<(&'static str, Vec<String>)>;

/// What installing or uninstalling did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Done {
    /// The file written or removed.
    pub file: PathBuf,
    /// Whether a definition was there before: replaced when installing, removed when
    /// uninstalling.
    pub was_installed: bool,
    /// The service manager's commands that ran, as lines for people.
    pub ran: Vec<String>,
    /// Commands that failed, with why, as lines for people.
    pub failed: Vec<String>,
}

/// Why the agent couldn't be installed or uninstalled.
#[derive(Debug)]
pub enum ServiceError {
    /// A file couldn't be written or removed.
    Io {
        /// The file.
        path: PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },
    /// The service manager refused to start it.
    Manager(String),
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Manager(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ServiceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Manager(_) => None,
        }
    }
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> ServiceError + '_ {
    move |source| ServiceError::Io {
        path: path.to_owned(),
        source,
    }
}

/// The owner of `path`, for launchd's domain.
#[cfg(unix)]
fn owner(path: &Path) -> Option<u32> {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::metadata(path).ok().map(|m| m.uid())
}

#[cfg(not(unix))]
fn owner(_: &Path) -> Option<u32> {
    None
}

/// Writes `text` to `path` as `platform` reads it: UTF-16 with a byte-order mark for
/// Task Scheduler, UTF-8 otherwise.
fn write(platform: Platform, path: &Path, text: &str) -> Result<(), ServiceError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io(dir))?;
    }
    let bytes: Vec<u8> = if platform == Platform::TaskScheduler {
        std::iter::once(0xFEFF_u16)
            .chain(text.encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect()
    } else {
        text.as_bytes().to_vec()
    };
    std::fs::write(path, bytes).map_err(io(path))
}

/// Reads a definition as [`install`] writes it.
#[must_use]
pub fn read(platform: Platform, path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if platform == Platform::TaskScheduler {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        let text = String::from_utf16_lossy(&units);
        Some(text.trim_start_matches('\u{feff}').to_owned())
    } else {
        String::from_utf8(bytes).ok()
    }
}

/// Runs `commands` with `manager`, noting each in `done`; the first failure stops them when
/// `stop_on_failure`.
fn run_all(
    manager: &mut dyn Manager,
    commands: &Commands,
    done: &mut Done,
    stop_on_failure: bool,
) -> Result<(), String> {
    for (program, args) in commands {
        let line = command_line(program, args);
        match manager.run(program, args) {
            Ok(()) => done.ran.push(line),
            Err(e) => {
                done.failed.push(format!("{line}: {e}"));
                if stop_on_failure {
                    return Err(e);
                }
            }
        }
    }
    Ok(())
}

/// Installs `definition` as `platform`'s service in `places` and starts it, replacing one
/// installed before.
///
/// # Errors
///
/// [`ServiceError`] if the file can't be written or the service manager won't start it.
pub fn install(
    platform: Platform,
    places: &Places,
    definition: &Definition,
    manager: &mut dyn Manager,
) -> Result<Done, ServiceError> {
    let file = places.file(platform);
    let mut done = Done {
        file: file.clone(),
        was_installed: file.exists(),
        ..Done::default()
    };
    let uid = owner(&places.home);
    let (start, stop) = commands(platform, &file, uid);
    if done.was_installed {
        // Stopped first, so it starts again with what's written now; it may not be running.
        let mut stopping = Done::default();
        let _ = run_all(manager, &stop, &mut stopping, false);
        done.ran.extend(stopping.ran);
    }
    if platform == Platform::Launchd {
        // The log's folder must be there before launchd opens it.
        if let Some(dir) = definition.log.parent() {
            std::fs::create_dir_all(dir).map_err(io(dir))?;
        }
    }
    write(platform, &file, &definition.contents(platform))?;
    run_all(manager, &start, &mut done, true).map_err(ServiceError::Manager)?;
    Ok(done)
}

/// Stops `platform`'s service and removes exactly what [`install`] wrote. Nothing is
/// touched when it isn't installed.
///
/// # Errors
///
/// [`ServiceError`] if the file can't be removed.
pub fn uninstall(
    platform: Platform,
    places: &Places,
    manager: &mut dyn Manager,
) -> Result<Done, ServiceError> {
    let file = places.file(platform);
    let mut done = Done {
        file: file.clone(),
        was_installed: file.exists(),
        ..Done::default()
    };
    if !done.was_installed {
        return Ok(done);
    }
    let (_, stop) = commands(platform, &file, owner(&places.home));
    let _ = run_all(manager, &stop, &mut done, false);
    std::fs::remove_file(&file).map_err(io(&file))?;
    if platform == Platform::Systemd {
        let reload = vec![(
            "systemctl",
            vec!["--user".to_owned(), "daemon-reload".to_owned()],
        )];
        let _ = run_all(manager, &reload, &mut done, false);
    }
    Ok(done)
}

/// What's installed for `platform` in `places`: its file and the archives it follows, or
/// `None` if nothing is.
#[must_use]
pub fn installed(platform: Platform, places: &Places) -> Option<(PathBuf, Vec<PathBuf>)> {
    let file = places.file(platform);
    let contents = read(platform, &file)?;
    Some((file, archives_in(platform, &contents)))
}

#[cfg(test)]
mod tests;
