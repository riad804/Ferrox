//! Host-side **dynamic plugin loading** (desktop only, `dynamic-plugins`
//! feature). Loads a shared library, reads its [`FerroxPluginV1`] descriptor,
//! and adapts it into a native [`Plugin`] + [`VideoEffectPlugin`] by marshalling
//! frames/params across the stable C ABI in [`super::abi`].
//!
//! Safety rests on the ABI contract: version-checked descriptor, plugin-owned
//! output buffers (copied out before the plugin frees them), and the plugin
//! promising thread-safety + no cross-boundary unwinding.

use std::ffi::CString;
use std::sync::Arc;

use serde_json::Value;

use crate::frame::{Frame, PixelFormat};

use super::abi::{FerroxPluginV1, FfiBuffer, ENTRY_SYMBOL, PLUGIN_ABI_VERSION};
use super::error::{PluginError, Result};
use super::metadata::PluginMetadata;
use super::traits::{Plugin, VideoEffectPlugin};

/// A native plugin backed by a dynamically-loaded C-ABI descriptor.
pub struct DynamicPlugin {
    descriptor: *const FerroxPluginV1,
    metadata: PluginMetadata,
    // Dropped last (after `Drop` runs `destroy`) to keep the library mapped
    // while the descriptor is still in use.
    _lib: Option<libloading::Library>,
}

// SAFETY: the loaded plugin promises its instance + function pointers are
// thread-safe (part of the ABI contract); the descriptor pointer is stable for
// the library's lifetime, which we keep alive in `_lib`.
unsafe impl Send for DynamicPlugin {}
unsafe impl Sync for DynamicPlugin {}

impl DynamicPlugin {
    /// Adapt a descriptor pointer into a plugin, validating the ABI version and
    /// parsing metadata. `keep_alive` holds the owning library, if any.
    ///
    /// # Safety
    /// `descriptor` must point to a valid [`FerroxPluginV1`] that outlives the
    /// returned plugin (guaranteed by `keep_alive` for loaded libraries).
    pub unsafe fn from_descriptor(descriptor: *const FerroxPluginV1, keep_alive: Option<libloading::Library>) -> Result<Self> {
        if descriptor.is_null() {
            return Err(PluginError::Other("plugin entry returned null".into()));
        }
        let desc = &*descriptor;
        if desc.abi_version != PLUGIN_ABI_VERSION {
            return Err(PluginError::Incompatible {
                id: "<dynamic>".into(),
                required: desc.abi_version.to_string(),
                host: PLUGIN_ABI_VERSION.to_string(),
            });
        }
        let json = std::ffi::CStr::from_ptr(desc.metadata_json)
            .to_str()
            .map_err(|e| PluginError::Other(format!("plugin metadata not UTF-8: {e}")))?;
        let metadata: PluginMetadata =
            serde_json::from_str(json).map_err(|e| PluginError::Other(format!("plugin metadata: {e}")))?;
        Ok(Self { descriptor, metadata, _lib: keep_alive })
    }

    fn desc(&self) -> &FerroxPluginV1 {
        // SAFETY: validated non-null in `from_descriptor`; library kept alive.
        unsafe { &*self.descriptor }
    }
}

impl Drop for DynamicPlugin {
    fn drop(&mut self) {
        let d = self.desc();
        // SAFETY: destroy the instance before `_lib` unloads (fields drop after).
        (d.destroy)(d.instance);
    }
}

impl Plugin for DynamicPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }
    fn as_video_effect(&self) -> Option<&dyn VideoEffectPlugin> {
        Some(self)
    }
}

impl VideoEffectPlugin for DynamicPlugin {
    fn apply_video(&self, frame: Frame, params: &Value) -> Result<Frame> {
        if frame.format != PixelFormat::Rgba8 {
            return Err(PluginError::Other(format!("dynamic video effect needs Rgba8, got {:?}", frame.format)));
        }
        let params_c = CString::new(params.to_string())
            .map_err(|e| PluginError::Other(format!("params NUL byte: {e}")))?;
        let d = self.desc();
        let mut out = FfiBuffer::empty();

        // SAFETY: input is a valid borrowed slice; `out` is a valid writable
        // slot; the plugin writes an owned buffer we copy then free below.
        let code = (d.apply_video)(
            d.instance,
            frame.width,
            frame.height,
            frame.data.as_ptr(),
            frame.data.len(),
            params_c.as_ptr(),
            &mut out,
        );
        if code != 0 {
            return Err(PluginError::Other(format!("plugin apply_video failed (code {code})")));
        }

        // Copy the plugin's buffer into a host-owned Vec, then let the plugin
        // free its own allocation (never cross allocators).
        let data = if out.ptr.is_null() {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(out.ptr, out.len) }.to_vec()
        };
        (d.free_buffer)(out);

        let expected = (frame.width as usize) * (frame.height as usize) * 4;
        if data.len() != expected {
            return Err(PluginError::Other(format!("plugin returned {} bytes, expected {expected}", data.len())));
        }
        Ok(Frame::new(frame.width, frame.height, PixelFormat::Rgba8, data))
    }
}

/// Load a plugin from a shared library at `path` (`.so`/`.dylib`/`.dll`).
///
/// # Safety consideration
/// Loading native code is inherently unsafe; only load libraries you trust.
pub fn load_plugin(path: impl AsRef<std::ffi::OsStr>) -> Result<Arc<dyn Plugin>> {
    // SAFETY: dlopen + symbol lookup; the returned descriptor is validated.
    unsafe {
        let lib = libloading::Library::new(path.as_ref())
            .map_err(|e| PluginError::Other(format!("dlopen failed: {e}")))?;
        let entry: libloading::Symbol<extern "C" fn() -> *const FerroxPluginV1> = lib
            .get(ENTRY_SYMBOL)
            .map_err(|e| PluginError::Other(format!("missing entry symbol '{}': {e}", String::from_utf8_lossy(&ENTRY_SYMBOL[..ENTRY_SYMBOL.len() - 1]))))?;
        let descriptor = entry();
        let plugin = DynamicPlugin::from_descriptor(descriptor, Some(lib))?;
        Ok(Arc::new(plugin))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{c_char, c_void};

    // A minimal in-process C-ABI plugin (grayscale) — proves the host adapter +
    // marshalling without needing a separately-compiled `.so`.
    extern "C" fn t_apply(
        _inst: *mut c_void,
        _w: u32,
        _h: u32,
        data: *const u8,
        len: usize,
        _params: *const c_char,
        out: *mut FfiBuffer,
    ) -> i32 {
        let input = unsafe { std::slice::from_raw_parts(data, len) };
        let mut v = input.to_vec();
        for px in v.chunks_exact_mut(4) {
            let g = ((px[0] as u32 + px[1] as u32 + px[2] as u32) / 3) as u8;
            px[0] = g;
            px[1] = g;
            px[2] = g;
        }
        unsafe { *out = FfiBuffer::from_vec(v) };
        0
    }
    extern "C" fn t_free(buf: FfiBuffer) {
        drop(unsafe { buf.into_vec() });
    }
    extern "C" fn t_destroy(_inst: *mut c_void) {}

    #[test]
    fn host_adapter_wraps_and_runs_a_cabi_plugin() {
        let meta = c"{\"id\":\"test.dyn\",\"name\":\"Dyn\",\"version\":{\"major\":1,\"minor\":0,\"patch\":0},\"kind\":\"video_effect\",\"api_version\":{\"major\":1,\"minor\":0,\"patch\":0}}";
        let desc = FerroxPluginV1 {
            abi_version: PLUGIN_ABI_VERSION,
            metadata_json: meta.as_ptr(),
            instance: std::ptr::null_mut(),
            apply_video: t_apply,
            free_buffer: t_free,
            destroy: t_destroy,
        };
        let plugin = unsafe { DynamicPlugin::from_descriptor(&desc, None) }.unwrap();
        assert_eq!(plugin.metadata().id, "test.dyn");

        let ve = plugin.as_video_effect().unwrap();
        let frame = Frame::new(1, 1, PixelFormat::Rgba8, vec![30, 150, 60, 255]);
        let out = ve.apply_video(frame, &Value::Null).unwrap();
        // grayscale of (30,150,60) = 80.
        assert_eq!(&out.data[..3], &[80, 80, 80]);
        assert_eq!(out.data[3], 255);
    }

    #[test]
    fn rejects_wrong_abi_version() {
        let meta = c"{}";
        let desc = FerroxPluginV1 {
            abi_version: 999,
            metadata_json: meta.as_ptr(),
            instance: std::ptr::null_mut(),
            apply_video: t_apply,
            free_buffer: t_free,
            destroy: t_destroy,
        };
        let res = unsafe { DynamicPlugin::from_descriptor(&desc, None) };
        assert!(matches!(res, Err(PluginError::Incompatible { .. })));
    }
}
