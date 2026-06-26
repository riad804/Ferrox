//! Standalone UniFFI bindings generator.
//!
//! Used by the platform build scripts to produce Kotlin and Swift bindings from
//! the compiled `ferrox-mobile` library, e.g.:
//!
//! ```sh
//! cargo run -p ferrox-mobile --bin uniffi-bindgen -- \
//!     generate --library target/.../libferrox_mobile.so \
//!     --language kotlin --out-dir bindings/kotlin
//! ```
fn main() {
    uniffi::uniffi_bindgen_main()
}
