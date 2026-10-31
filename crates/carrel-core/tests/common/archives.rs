//! Making small compressed files for tests.

use std::io::Write as _;
use std::path::Path;

/// One entry to put in a test archive.
pub enum Item<'a> {
    /// A file with these contents.
    File(&'a str, &'a [u8]),
    /// An executable file.
    Exec(&'a str, &'a [u8]),
    /// A folder.
    Dir(&'a str),
    /// A symbolic link to the target.
    Link(&'a str, &'a str),
}

/// Writes a zip of `items` at `path`, every entry dated 2024-05-06 07:08.
pub fn zip(path: &Path, items: &[Item<'_>]) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let when = zip::DateTime::from_date_and_time(2024, 5, 6, 7, 8, 0).unwrap();
    let options = zip::write::SimpleFileOptions::default().last_modified_time(when);
    for item in items {
        match item {
            Item::File(name, bytes) => {
                zip.start_file(*name, options).unwrap();
                zip.write_all(bytes).unwrap();
            }
            Item::Exec(name, bytes) => {
                zip.start_file(*name, options.unix_permissions(0o755))
                    .unwrap();
                zip.write_all(bytes).unwrap();
            }
            Item::Dir(name) => zip.add_directory(*name, options).unwrap(),
            Item::Link(name, target) => zip.add_symlink(*name, *target, options).unwrap(),
        }
    }
    zip.finish().unwrap();
}

/// A tar of `items`, every entry dated 2024-05-06.
pub fn tar(items: &[Item<'_>]) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    for item in items {
        let mut header = tar::Header::new_gnu();
        header.set_mtime(1_714_979_280);
        match item {
            Item::File(name, bytes) | Item::Exec(name, bytes) => {
                header.set_size(bytes.len() as u64);
                header.set_mode(if matches!(item, Item::Exec(..)) {
                    0o755
                } else {
                    0o644
                });
                header.set_entry_type(tar::EntryType::Regular);
                tar.append_data(&mut header, name, *bytes).unwrap();
            }
            Item::Dir(name) => {
                header.set_size(0);
                header.set_mode(0o755);
                header.set_entry_type(tar::EntryType::Directory);
                tar.append_data(&mut header, name, std::io::empty())
                    .unwrap();
            }
            Item::Link(name, target) => {
                header.set_size(0);
                header.set_entry_type(tar::EntryType::Symlink);
                tar.append_link(&mut header, name, target).unwrap();
            }
        }
    }
    tar.into_inner().unwrap()
}

/// `bytes` gzipped.
pub fn gz(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

/// `bytes` in bzip2.
pub fn bz2(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

/// `bytes` in xz.
pub fn xz(bytes: &[u8]) -> Vec<u8> {
    let mut writer =
        lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::with_preset(1)).unwrap();
    writer.write_all(bytes).unwrap();
    writer.finish().unwrap()
}

/// `bytes` in zstd.
pub fn zst(bytes: &[u8]) -> Vec<u8> {
    ruzstd::encoding::compress_to_vec(bytes, ruzstd::encoding::CompressionLevel::Fastest)
}
