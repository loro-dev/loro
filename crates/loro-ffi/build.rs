fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let config = cbindgen::Config::from_file("cbindgen.toml")
        .expect("Unable to find cbindgen.toml configuration file");
    cbindgen::generate_with_config(crate_dir, config)
        .unwrap()
        .write_to_file("target/loro_ffi.h");
}
