//! Swift bindings for `carrel-core`.
//!
//! `UniFFI` bindings and the `XCFramework` build arrive with #20. Until then this crate only
//! re-exports the version information the apps will show.

/// The Carrel version and the archive format version, as shown in the apps' About window.
#[must_use]
pub fn version_summary() -> String {
    format!(
        "Carrel {} · archive format {}",
        carrel_core::CARREL_VERSION,
        carrel_core::FORMAT_VERSION
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn summary_names_both_versions() {
        let s = super::version_summary();
        assert!(s.contains(carrel_core::CARREL_VERSION));
        assert!(s.contains("archive format 1.0"));
    }
}
