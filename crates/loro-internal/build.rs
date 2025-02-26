use std::env;
use std::fs;
use std::path::Path;

fn main() {
    // Tell Cargo to re-run this script if package.json changes
    println!("cargo:rerun-if-changed=../loro-wasm/package.json");

    // Read the package.json file from the parent directory
    let pkg_json =
        fs::read_to_string("../loro-wasm/package.json").expect("Failed to read package.json");

    // Extract the version
    let version = extract_version(&pkg_json);

    // Get the output directory from Cargo
    let out_dir = env::var("OUT_DIR").expect("Failed to get OUT_DIR");

    // Write the version to a file in the output directory
    let version_path = Path::new(&out_dir).join("version.txt");
    eprintln!("loro-crdt version: {}", version);
    fs::write(version_path, version).expect("Failed to write version to file");
}

fn extract_version(pkg_json: &str) -> String {
    // Use serde_json for more robust parsing
    let parsed: serde_json::Value =
        serde_json::from_str(pkg_json).expect("Failed to parse package.json");
    parsed["version"]
        .as_str()
        .expect("Failed to find version in package.json")
        .to_string()
}
