use std::io::Write;

fn main() {
    arm_targets::process();
    write("z7link.x", include_bytes!("z7link.x"));
}

fn write(file: &str, contents: &[u8]) {
    // Put the linker script in our output directory and ensure it's on the linker search path.
    let out = &std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::File::create(out.join(file))
        .unwrap()
        .write_all(contents)
        .unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed={file}");
}
