//! The **stable C-ABI plugin contract** — the only safe way to load a plugin
//! from a shared library, given Rust has no stable ABI.
//!
//! A plugin library exports one `extern "C"` entry symbol
//! ([`ENTRY_SYMBOL`]) returning a pointer to a [`FerroxPluginV1`] descriptor: a
//! `#[repr(C)]` struct of metadata + function pointers. All data crosses as
//! C-compatible types — raw byte buffers ([`FfiBuffer`]) and null-terminated
//! JSON strings — never Rust `std` types or trait objects.
//!
//! These types are always compiled (no `libloading`), so a plugin **author**
//! depends only on `ferrox-core` to build a descriptor. The **host** loader
//! lives behind the `dynamic-plugins` feature ([`super::dynamic`]).
//!
//! ## Rules (both sides must honour)
//! - Check [`FerroxPluginV1::abi_version`] == [`PLUGIN_ABI_VERSION`] before use.
//! - The plugin **allocates** output buffers and **frees** them via its own
//!   `free_buffer` (never cross allocators). The host copies out first.
//! - Plugin fns must **not unwind** across the boundary — wrap bodies in
//!   `catch_unwind` and return a non-zero error code instead.

use std::ffi::{c_char, c_void};

/// The ABI version. Bump on **any** layout/semantics change to
/// [`FerroxPluginV1`]; the host refuses to load a mismatched plugin.
pub const PLUGIN_ABI_VERSION: u32 = 1;

/// The entry symbol every dynamic plugin library must export as
/// `#[no_mangle] pub extern "C" fn ferrox_plugin_v1() -> *const FerroxPluginV1`.
pub const ENTRY_SYMBOL: &[u8] = b"ferrox_plugin_v1\0";

/// A C-compatible owned byte buffer. Whoever allocates it also frees it (via the
/// plugin's `free_buffer`), so allocators never cross the boundary.
#[repr(C)]
pub struct FfiBuffer {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

impl FfiBuffer {
    /// An empty buffer (null).
    pub fn empty() -> Self {
        Self { ptr: std::ptr::null_mut(), len: 0, cap: 0 }
    }

    /// Hand ownership of `v`'s allocation to a raw buffer. Reclaim it later with
    /// [`FfiBuffer::into_vec`] **in the same crate/allocator**.
    pub fn from_vec(v: Vec<u8>) -> Self {
        let mut v = std::mem::ManuallyDrop::new(v);
        Self { ptr: v.as_mut_ptr(), len: v.len(), cap: v.capacity() }
    }

    /// # Safety
    /// Must be called exactly once, in the same allocator that produced it via
    /// [`FfiBuffer::from_vec`].
    pub unsafe fn into_vec(self) -> Vec<u8> {
        if self.ptr.is_null() {
            Vec::new()
        } else {
            Vec::from_raw_parts(self.ptr, self.len, self.cap)
        }
    }
}

/// Apply a video effect. `data`/`data_len` is borrowed RGBA8 input (host-owned);
/// `params_json` is a borrowed null-terminated JSON string; the plugin writes an
/// allocated RGBA8 output into `*out`. Returns `0` on success.
pub type ApplyVideoFn = extern "C" fn(
    instance: *mut c_void,
    width: u32,
    height: u32,
    data: *const u8,
    data_len: usize,
    params_json: *const c_char,
    out: *mut FfiBuffer,
) -> i32;

/// Free a buffer the plugin allocated.
pub type FreeBufferFn = extern "C" fn(FfiBuffer);

/// Destroy the plugin instance.
pub type DestroyFn = extern "C" fn(*mut c_void);

/// The `#[repr(C)]` descriptor a dynamic plugin exports (ABI v1). Currently
/// models a **video-effect** plugin; later ABI versions add more kinds.
#[repr(C)]
pub struct FerroxPluginV1 {
    /// Must equal [`PLUGIN_ABI_VERSION`].
    pub abi_version: u32,
    /// Null-terminated JSON of the plugin's `PluginMetadata`; valid for the
    /// plugin's lifetime.
    pub metadata_json: *const c_char,
    /// Opaque plugin state, passed back to the function pointers.
    pub instance: *mut c_void,
    pub apply_video: ApplyVideoFn,
    pub free_buffer: FreeBufferFn,
    pub destroy: DestroyFn,
}
