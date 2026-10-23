use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn rust_sources(directory: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(directory).expect("read source directory") {
        let path = entry.expect("source entry").path();
        if path.is_dir() {
            rust_sources(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn main() {
    let mut files = vec!["Cargo.toml".into(), "Cargo.lock".into(), "build.rs".into()];
    rust_sources(Path::new("src"), &mut files);
    files.sort();
    let mut hash = Sha256::new();
    for file in files {
        println!("cargo:rerun-if-changed={}", file.display());
        let name = file.to_string_lossy().replace('\\', "/");
        let bytes = fs::read(&file).expect("read fingerprint source");
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    println!("cargo:rerun-if-changed=src");
    for key in ["TARGET", "PROFILE", "CARGO_CFG_TARGET_FEATURE"] {
        hash.update(std::env::var(key).unwrap_or_default().as_bytes());
        hash.update([0]);
    }
    println!("cargo:rustc-env=PAIGU_COMPUTE_BUILD={:x}", hash.finalize());
}
