//! Reference dynamic plugin: an **invert-colours** video effect, exported over
//! the stable ferrox C-ABI ([`ferrox_core::plugin::abi`]).
//!
//! Build as a shared library and load with `ferrox_core::plugin::load_plugin`.
//! This is the template a third-party plugin author follows: implement the
//! `extern "C"` functions, wrap bodies in `catch_unwind` (unwinding across the
//! boundary is UB), and export the `ferrox_plugin_v1` entry symbol.

use std::ffi::{c_char, c_void, CStr};
use std::panic::catch_unwind;
use std::sync::OnceLock;

use ferrox_core::plugin::abi::{FerroxPluginV1, FfiBuffer, PLUGIN_ABI_VERSION};

/// Plugin metadata as a static null-terminated JSON `PluginMetadata`.
static METADATA: &CStr = c"{\"id\":\"ferrox.example.invert\",\"name\":\"Invert\",\"version\":{\"major\":1,\"minor\":0,\"patch\":0},\"kind\":\"video_effect\",\"api_version\":{\"major\":1,\"minor\":0,\"patch\":0},\"author\":\"example\",\"description\":\"inverts RGB\"}";

extern "C" fn apply_video(
    _instance: *mut c_void,
    _width: u32,
    _height: u32,
    data: *const u8,
    len: usize,
    _params_json: *const c_char,
    out: *mut FfiBuffer,
) -> i32 {
    let result = catch_unwind(|| {
        // SAFETY: host guarantees `data`/`len` is a valid borrowed RGBA slice.
        let input = unsafe { std::slice::from_raw_parts(data, len) };
        let mut v = input.to_vec();
        for px in v.chunks_exact_mut(4) {
            px[0] = 255 - px[0];
            px[1] = 255 - px[1];
            px[2] = 255 - px[2];
            // alpha unchanged
        }
        v
    });
    match result {
        Ok(v) => {
            // SAFETY: `out` is a valid writable slot provided by the host.
            unsafe { *out = FfiBuffer::from_vec(v) };
            0
        }
        Err(_) => 1, // never unwind across the boundary
    }
}

extern "C" fn free_buffer(buf: FfiBuffer) {
    // SAFETY: reclaim in the same (this crate's) allocator that produced it.
    drop(unsafe { buf.into_vec() });
}

extern "C" fn destroy(_instance: *mut c_void) {}

/// The entry symbol the host looks up. Returns a stable descriptor pointer.
///
/// # Safety
/// Called by the host loader across the FFI boundary.
#[no_mangle]
pub extern "C" fn ferrox_plugin_v1() -> *const FerroxPluginV1 {
    static DESC: OnceLock<usize> = OnceLock::new();
    let ptr = DESC.get_or_init(|| {
        let desc = Box::new(FerroxPluginV1 {
            abi_version: PLUGIN_ABI_VERSION,
            metadata_json: METADATA.as_ptr(),
            instance: std::ptr::null_mut(),
            apply_video,
            free_buffer,
            destroy,
        });
        Box::into_raw(desc) as usize
    });
    *ptr as *const FerroxPluginV1
}
