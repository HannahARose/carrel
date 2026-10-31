//! Builds the `OpenCascade` shim when `OpenCascade` is installed, and sets `occt`. Without it the
//! provider still builds, and offers STEP and IGES drawing as unavailable, with how to install
//! `OpenCascade`.

use std::path::{Path, PathBuf};

/// Where package managers put `OpenCascade`: Homebrew on Apple silicon and Intel, then Linux.
const PREFIXES: [&str; 4] = [
    "/opt/homebrew/opt/opencascade",
    "/usr/local/opt/opencascade",
    "/usr",
    "/usr/local",
];

/// The libraries the shim uses, before the data exchange ones, which were renamed in 7.8.
const LIBRARIES: [&str; 11] = [
    "TKMesh",
    "TKTopAlgo",
    "TKGeomAlgo",
    "TKGeomBase",
    "TKBRep",
    "TKG3d",
    "TKG2d",
    "TKShHealing",
    "TKMath",
    "TKernel",
    "TKXSBase",
];

fn main() {
    println!("cargo::rustc-check-cfg=cfg(occt)");
    println!("cargo::rerun-if-changed=shim/carrel_cad.cpp");
    println!("cargo::rerun-if-env-changed=OCCT_INCLUDE_DIR");
    println!("cargo::rerun-if-env-changed=OCCT_LIB_DIR");
    let Some((include, lib)) = find() else {
        println!(
            "cargo::warning=OpenCascade isn't installed: carrel-provider-cad won't draw STEP or IGES"
        );
        return;
    };
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .file("shim/carrel_cad.cpp")
        .include(&include)
        .warnings(false)
        .compile("carrel_cad");
    println!("cargo::rustc-link-search=native={}", lib.display());
    let exchange: &[&str] = if has(&lib, "TKDESTEP") {
        &["TKDESTEP", "TKDEIGES", "TKDE"]
    } else {
        &["TKSTEP", "TKSTEPAttr", "TKSTEP209", "TKSTEPBase", "TKIGES"]
    };
    for name in exchange.iter().chain(&LIBRARIES) {
        println!("cargo::rustc-link-lib=dylib={name}");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        // Linux keeps OpenCascade's libraries where the loader finds them; this is for others.
        println!("cargo::rustc-link-arg=-Wl,-rpath,{}", lib.display());
    }
    println!("cargo::rustc-cfg=occt");
}

/// `OpenCascade`'s headers and libraries: from `OCCT_INCLUDE_DIR` and `OCCT_LIB_DIR`, or the
/// first prefix that has both.
fn find() -> Option<(PathBuf, PathBuf)> {
    if let (Ok(include), Ok(lib)) = (
        std::env::var("OCCT_INCLUDE_DIR"),
        std::env::var("OCCT_LIB_DIR"),
    ) {
        return Some((include.into(), lib.into()));
    }
    PREFIXES.iter().find_map(|prefix| {
        let include = Path::new(prefix).join("include/opencascade");
        if !include.join("Standard.hxx").is_file() {
            return None;
        }
        let lib = [
            "lib",
            "lib/x86_64-linux-gnu",
            "lib/aarch64-linux-gnu",
            "lib64",
        ]
        .iter()
        .map(|l| Path::new(prefix).join(l))
        .find(|l| has(l, "TKernel"))?;
        Some((include, lib))
    })
}

/// Whether `dir` has the library `name`.
fn has(dir: &Path, name: &str) -> bool {
    ["dylib", "so", "lib"].iter().any(|ext| {
        dir.join(format!("lib{name}.{ext}")).exists() || dir.join(format!("{name}.{ext}")).exists()
    })
}
