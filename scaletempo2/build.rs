use std::env;
use std::path::PathBuf;

fn main() {
    // Tell cargo to invalidate the built crate whenever the C source changes
    println!("cargo:rerun-if-changed=c/scaletempo2_internal.c");
    println!("cargo:rerun-if-changed=c/scaletempo2_wrapper.c");
    println!("cargo:rerun-if-changed=c/scaletempo2.h");
    println!("cargo:rerun-if-changed=c/scaletempo2_internal.h");

    // Build the C library using cc crate
    cc::Build::new()
        .files(&[
            "c/scaletempo2_internal.c",
            "c/scaletempo2_wrapper.c",
        ])
        .include("c")
        .flag("-std=c11")
        .flag("-O2")
        .flag("-fPIC")
        .compile("scaletempo2");

    // Link with math library
    println!("cargo:rustc-link-lib=m");

    // Generate bindings
    let bindings = bindgen::Builder::default()
        .header("c/scaletempo2.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        // Generate bindings for functions and types we need
        .allowlist_function("mp_scaletempo2_.*")
        .allowlist_type("mp_scaletempo2.*")
        .allowlist_var("mp_scaletempo2.*")
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
