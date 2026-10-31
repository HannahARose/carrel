//! STEP and IGES files meshed through `OpenCascade`, by the C++ shim in `shim/carrel_cad.cpp`.
//! Without `OpenCascade` at build time, meshing reports it isn't installed.

use std::path::Path;

/// A meshed model.
#[derive(Debug, Default)]
pub struct Mesh {
    /// The corners, in millimetres.
    pub positions: Vec<[f32; 3]>,
    /// Each triangle's corners, as indices into `positions`.
    pub triangles: Vec<[u32; 3]>,
    /// How many solids it has.
    pub solids: usize,
    /// Their volume, in cubic millimetres.
    pub volume: f64,
}

/// How fine the mesh is: its largest gap from the true surface, as a share of the model's
/// diagonal. Coarse, as drawings at 512 pixels need.
#[cfg(occt)]
pub const DEFLECTION: f64 = 0.01;

/// What to tell someone without `OpenCascade`.
pub const INSTALL: &str = "OpenCascade isn't installed: brew install opencascade, or apt install libocct-data-exchange-dev libocct-modeling-algorithms-dev, then build carrel-provider-cad";

/// Whether this build can mesh models.
pub const AVAILABLE: bool = cfg!(occt);

/// The model in the file at `path`, an IGES file when `iges` is set and STEP otherwise.
#[cfg(occt)]
#[allow(unsafe_code)] // The shim's C interface; see the safety notes below.
pub fn mesh(path: &Path, iges: bool) -> Result<Mesh, String> {
    use std::ffi::{CStr, CString, c_char, c_int};

    #[repr(C)]
    struct CarrelMesh {
        positions: *mut f32,
        indices: *mut u32,
        vertex_count: usize,
        triangle_count: usize,
        solids: usize,
        volume: f64,
        error: *mut c_char,
    }

    unsafe extern "C" {
        fn carrel_cad_mesh(
            path: *const c_char,
            iges: c_int,
            fraction: f64,
            out: *mut CarrelMesh,
        ) -> c_int;
        fn carrel_cad_free(mesh: *mut CarrelMesh);
    }

    let path = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| "the path has a NUL in it".to_owned())?;
    let mut out = CarrelMesh {
        positions: std::ptr::null_mut(),
        indices: std::ptr::null_mut(),
        vertex_count: 0,
        triangle_count: 0,
        solids: 0,
        volume: 0.0,
        error: std::ptr::null_mut(),
    };
    // SAFETY: `path` is a valid C string for the call, and `out` a valid struct the shim fills.
    let status =
        unsafe { carrel_cad_mesh(path.as_ptr(), c_int::from(iges), DEFLECTION, &raw mut out) };
    let result = if status == 0 {
        // SAFETY: on success the shim allocated `vertex_count * 3` floats and
        // `triangle_count * 3` indices, which stay valid until `carrel_cad_free`.
        let (positions, indices) = unsafe {
            (
                std::slice::from_raw_parts(out.positions, out.vertex_count * 3),
                std::slice::from_raw_parts(out.indices, out.triangle_count * 3),
            )
        };
        Ok(Mesh {
            positions: positions.as_chunks::<3>().0.to_vec(),
            triangles: indices.as_chunks::<3>().0.to_vec(),
            solids: out.solids,
            volume: out.volume,
        })
    } else if out.error.is_null() {
        Err("OpenCascade failed".to_owned())
    } else {
        // SAFETY: the shim sets `error` to a NUL-terminated string it allocated.
        Err(unsafe { CStr::from_ptr(out.error) }
            .to_string_lossy()
            .into_owned())
    };
    // SAFETY: `out` was filled by `carrel_cad_mesh` and is freed once.
    unsafe { carrel_cad_free(&raw mut out) };
    result
}

/// Without `OpenCascade`, nothing can be meshed.
#[cfg(not(occt))]
pub fn mesh(_path: &Path, _iges: bool) -> Result<Mesh, String> {
    Err(INSTALL.to_owned())
}
