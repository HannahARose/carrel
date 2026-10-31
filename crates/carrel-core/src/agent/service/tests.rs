use super::*;

/// Records the commands it's asked to run, failing those named in `fail`.
#[derive(Default)]
struct Fake {
    ran: Vec<String>,
    fail: Vec<&'static str>,
}

impl Manager for Fake {
    fn run(&mut self, program: &str, args: &[String]) -> Result<(), String> {
        let line = command_line(program, args);
        self.ran.push(line.clone());
        if self.fail.iter().any(|f| line.contains(f)) {
            Err("refused".into())
        } else {
            Ok(())
        }
    }
}

fn places(dir: &Path) -> Places {
    Places {
        home: dir.join("home"),
        xdg_config_home: None,
        config: dir.join("home/config/carrel"),
    }
}

fn definition(places: &Places) -> Definition {
    Definition {
        program: PathBuf::from("/opt/carrel/bin/carrel"),
        archives: vec![
            PathBuf::from("/Users/me/Archive"),
            PathBuf::from("/Volumes/Drive/Owner's \"Papers\" & 100% $HOME"),
        ],
        environment: vec![("PATH".into(), "/opt/homebrew/bin:/usr/bin".into())],
        log: places.log(),
    }
}

const ALL: [Platform; 3] = [
    Platform::Launchd,
    Platform::Systemd,
    Platform::TaskScheduler,
];

#[test]
fn each_platform_writes_its_own_file_in_the_home_folder_given() {
    let dir = tempfile::tempdir().unwrap();
    let places = places(dir.path());
    let home = &places.home;
    assert_eq!(
        places.file(Platform::Launchd),
        home.join("Library/LaunchAgents/io.github.hannaharose.carrel.agent.plist")
    );
    assert_eq!(
        places.file(Platform::Systemd),
        home.join(".config/systemd/user/carrel-agent.service")
    );
    assert_eq!(
        places.file(Platform::TaskScheduler),
        places.config.join("carrel-agent.xml")
    );
    let xdg = Places {
        xdg_config_home: Some(dir.path().join("xdg")),
        ..places.clone()
    };
    assert_eq!(
        xdg.file(Platform::Systemd),
        dir.path().join("xdg/systemd/user/carrel-agent.service")
    );
}

#[test]
fn the_archives_are_read_back_from_each_definition_whatever_their_names() {
    let dir = tempfile::tempdir().unwrap();
    let places = places(dir.path());
    let definition = definition(&places);
    for platform in ALL {
        let text = definition.contents(platform);
        assert_eq!(
            archives_in(platform, &text),
            definition.archives,
            "{platform:?}:\n{text}"
        );
    }
}

#[test]
fn the_definitions_say_what_each_service_manager_needs() {
    let dir = tempfile::tempdir().unwrap();
    let places = places(dir.path());
    let definition = definition(&places);

    let plist = definition.contents(Platform::Launchd);
    assert!(plist.contains("<string>io.github.hannaharose.carrel.agent</string>"));
    assert!(plist.contains("<key>RunAtLoad</key>\n  <true/>"));
    assert!(plist.contains("<key>PATH</key>\n    <string>/opt/homebrew/bin:/usr/bin</string>"));
    assert!(plist.contains("&amp; 100%"), "XML is escaped");

    let unit = definition.contents(Platform::Systemd);
    assert!(unit.contains("ExecStart=\"/opt/carrel/bin/carrel\" \"agent\" \"/Users/me/Archive\""));
    assert!(
        unit.contains("100%% $$HOME"),
        "systemd's specials are escaped"
    );
    assert!(unit.contains("Environment=\"PATH=/opt/homebrew/bin:/usr/bin\""));
    assert!(unit.contains("WantedBy=default.target"));

    let task = definition.contents(Platform::TaskScheduler);
    assert!(task.contains("<LogonTrigger>"));
    assert!(task.contains("<LogonType>InteractiveToken</LogonType>"));
    assert!(task.contains("-WindowStyle Hidden"));
    assert!(task.contains("Owner''s"), "PowerShell quotes are doubled");
}

#[test]
fn installing_writes_the_file_and_starts_it_and_uninstalling_removes_exactly_that() {
    for platform in ALL {
        let dir = tempfile::tempdir().unwrap();
        let places = places(dir.path());
        let definition = definition(&places);
        let mut manager = Fake::default();
        assert_eq!(installed(platform, &places), None);

        let done = install(platform, &places, &definition, &mut manager).unwrap();
        assert!(!done.was_installed);
        assert_eq!(done.file, places.file(platform));
        assert!(!done.ran.is_empty() && done.failed.is_empty());
        assert_eq!(done.ran, manager.ran, "{platform:?}");
        assert_eq!(
            installed(platform, &places),
            Some((done.file.clone(), definition.archives.clone()))
        );

        let again = install(platform, &places, &definition, &mut manager).unwrap();
        assert!(again.was_installed, "replaced");

        manager.ran.clear();
        let removed = uninstall(platform, &places, &mut manager).unwrap();
        assert!(removed.was_installed);
        assert!(!done.file.exists());
        assert!(!manager.ran.is_empty());
        assert_eq!(installed(platform, &places), None);

        manager.ran.clear();
        let nothing = uninstall(platform, &places, &mut manager).unwrap();
        assert!(!nothing.was_installed);
        assert_eq!(manager.ran, Vec::<String>::new(), "nothing to stop");
    }
}

#[test]
fn the_service_managers_commands_are_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let places = places(dir.path());
    let definition = definition(&places);
    let mut manager = Fake::default();
    install(Platform::Systemd, &places, &definition, &mut manager).unwrap();
    assert_eq!(
        manager.ran,
        [
            "systemctl --user daemon-reload",
            "systemctl --user enable --now carrel-agent.service"
        ]
    );
    let mut manager = Fake::default();
    install(Platform::TaskScheduler, &places, &definition, &mut manager).unwrap();
    assert!(manager.ran[0].starts_with("schtasks /Create /TN \"Carrel Agent\" /XML "));
    let mut manager = Fake::default();
    let done = install(Platform::Launchd, &places, &definition, &mut manager).unwrap();
    assert!(done.ran[0].starts_with("launchctl bootstrap gui/"));
    assert!(
        places.log().parent().unwrap().is_dir(),
        "launchd's log folder"
    );
}

#[test]
fn a_service_manager_that_refuses_is_an_error_and_the_file_stays_to_uninstall() {
    let dir = tempfile::tempdir().unwrap();
    let places = places(dir.path());
    let mut manager = Fake {
        fail: vec!["enable"],
        ..Fake::default()
    };
    let error = install(
        Platform::Systemd,
        &places,
        &definition(&places),
        &mut manager,
    )
    .unwrap_err();
    assert!(matches!(error, ServiceError::Manager(_)));
    assert!(places.file(Platform::Systemd).exists());
    assert!(
        uninstall(Platform::Systemd, &places, &mut manager)
            .unwrap()
            .was_installed
    );
}
