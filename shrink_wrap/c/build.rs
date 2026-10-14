// Builds shrink_wrap.c with the flags `just lint` uses, warnings are errors.
fn main() {
    println!("cargo::rerun-if-changed=shrink_wrap.c");
    println!("cargo::rerun-if-changed=shrink_wrap.h");
    println!("cargo::rerun-if-changed=src/sizes.c");
    cc::Build::new()
        .file("shrink_wrap.c")
        .file("src/sizes.c")
        .include(".")
        .std("c99")
        .flag("-pedantic")
        .flag("-Wconversion")
        .flag("-Wsign-conversion")
        .warnings(true)
        .extra_warnings(true)
        .warnings_into_errors(true)
        .compile("shrink_wrap_c");
}
