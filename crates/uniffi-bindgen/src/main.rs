//! `uniffi-bindgen`, pinned to the `UniFFI` version `carrel-ffi` uses.
//!
//! Run by `scripts/build-xcframework.sh`; see `uniffi-bindgen --help`.

fn main() {
    uniffi::uniffi_bindgen_main();
}
