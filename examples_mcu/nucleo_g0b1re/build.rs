use std::path::PathBuf;
use std::{env, fs};

fn main() {
    // Put `memory.x` in our output directory and ensure it's on the linker search path.
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("memory.x"), include_bytes!("memory.x")).unwrap();

    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rustc-link-arg=-Tlink.x"); // provided by cortex-m-rt

    println!("cargo:rustc-link-arg=-Tdefmt.x");
    println!("cargo:rustc-linker=flip-link");
}
