//! Backups with the real `kopia` program. Skipped when Kopia isn't installed, unless
//! `CARREL_REQUIRE_KOPIA` is set, as it is in CI.

use carrel_core::archive::{Archive, InitOptions};
use carrel_core::backup::{BackupError, Backups, Kopia, ignore_patterns};
use carrel_core::card::Id;

#[test]
fn set_up_snapshot_list_verify_and_restore() {
    if Kopia::program().is_none() {
        assert!(
            std::env::var_os("CARREL_REQUIRE_KOPIA").is_none(),
            "Kopia is required here but isn't installed"
        );
        eprintln!("skipped: kopia isn't installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let archive =
        Archive::init(&dir.path().join("Archive"), &InitOptions::new("Kopia test")).unwrap();
    let item = archive.root().join("Holding/lamp-build");
    std::fs::create_dir_all(item.join("node_modules/dep")).unwrap();
    std::fs::write(item.join("notes.md"), "lamp notes\n").unwrap();
    std::fs::write(item.join("node_modules/dep/index.js"), "regenerable\n").unwrap();
    assert!(ignore_patterns(&archive).contains(&"node_modules/".to_owned()));

    let config = dir.path().join("config/kopia.config");
    let repository = dir.path().join("repo");
    let password = Id::new_v7().to_string();
    let inside = Kopia::setup(
        &archive,
        config.clone(),
        &archive.root().join("repo"),
        &password,
        false,
        Id::new_v7(),
    );
    assert!(
        matches!(inside, Err(BackupError::Refused(_))),
        "never inside the archive"
    );

    let (kopia, setup) = Kopia::setup(
        &archive,
        config.clone(),
        &repository,
        &password,
        false,
        Id::new_v7(),
    )
    .unwrap();
    assert!(setup.created);
    assert_eq!(setup.repository.len(), 64, "Kopia's repository identifier");

    let snapshot = kopia.snapshot(archive.root(), "first").unwrap();
    assert!(snapshot.pinned);
    let listed = kopia.list(archive.root()).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, snapshot.id);
    assert_eq!(listed[0].description.as_deref(), Some("first"));
    kopia.verify(100).unwrap();

    let restored = dir.path().join("restored");
    kopia
        .restore(&snapshot.id, "Holding/lamp-build", &restored)
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(restored.join("notes.md")).unwrap(),
        "lamp notes\n"
    );
    assert!(
        !restored.join("node_modules").exists(),
        "regenerable folders aren't backed up"
    );
    assert!(
        matches!(
            kopia.restore(&snapshot.id, "Holding/lamp-build", &restored),
            Err(BackupError::Refused(_))
        ),
        "restore never overwrites"
    );

    // A snapshot can be forgotten, pinned as it is; the others stay.
    let second = kopia.snapshot(archive.root(), "second").unwrap();
    assert_eq!(kopia.list(archive.root()).unwrap().len(), 2);
    kopia.forget(&second.id).unwrap();
    let listed = kopia.list(archive.root()).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, snapshot.id);

    // Reconnecting with the wrong password fails clearly; the right one connects.
    let other = dir.path().join("config/other.config");
    assert!(matches!(
        Kopia::setup(
            &archive,
            other.clone(),
            &repository,
            "wrong",
            false,
            Id::new_v7()
        ),
        Err(BackupError::WrongPassword)
    ));
    let (_, again) =
        Kopia::setup(&archive, other, &repository, &password, false, Id::new_v7()).unwrap();
    assert!(!again.created);
    assert_eq!(again.repository, setup.repository);

    // A drive that's gone is reported as such.
    std::fs::rename(&repository, dir.path().join("unplugged")).unwrap();
    let reopened = Kopia::open(&archive, config)
        .unwrap()
        .with_password(password);
    assert!(matches!(
        reopened.snapshot(archive.root(), "second"),
        Err(BackupError::Unreachable(_))
    ));
}
