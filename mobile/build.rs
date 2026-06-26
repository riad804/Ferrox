fn main() {
    // Proc-macro-only UniFFI (no .udl file): `uniffi::setup_scaffolding!()` in
    // lib.rs emits the FFI scaffolding, so there is nothing to do at build time
    // beyond ensuring uniffi is linked. Kept as a build script anchor in case a
    // .udl is added later.
    println!("cargo:rerun-if-changed=src/lib.rs");
}
