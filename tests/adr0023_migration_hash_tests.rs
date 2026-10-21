//! ADR 0023: a committed migration is never changed.
//!
//! `tests/fixtures/migration_sha256.txt` gives the SHA-256 of each migration
//! file. A changed file fails the first test. A new file fails the second
//! test until its line is added at the end of the list.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn recorded_hashes() -> BTreeMap<String, String> {
    let path = manifest_dir().join("tests/fixtures/migration_sha256.txt");
    let text = std::fs::read_to_string(&path).expect("read the migration hash list");
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let (name, hash) = line
                .split_once(' ')
                .expect("each line gives a file name and a hash");
            (name.to_string(), hash.trim().to_string())
        })
        .collect()
}

fn migration_files() -> Vec<String> {
    let dir = manifest_dir().join("migrations");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("read migrations")
        .map(|entry| {
            entry
                .expect("read entry")
                .file_name()
                .into_string()
                .expect("utf-8 name")
        })
        .filter(|name| Path::new(name).extension().is_some_and(|ext| ext == "sql"))
        .collect();
    names.sort();
    names
}

fn file_hash(name: &str) -> String {
    let bytes =
        std::fs::read(manifest_dir().join("migrations").join(name)).expect("read migration");
    hex::encode(Sha256::digest(&bytes))
}

#[test]
fn each_committed_migration_is_unchanged() {
    for (name, recorded) in recorded_hashes() {
        assert_eq!(
            file_hash(&name),
            recorded,
            "ADR 0023: migrations/{name} changed after its commit. Restore the file, \
             and put the change in a new migration"
        );
    }
}

#[test]
fn each_migration_file_has_a_recorded_hash() {
    let recorded = recorded_hashes();
    for name in migration_files() {
        assert!(
            recorded.contains_key(&name),
            "ADR 0023: migrations/{name} has no line in tests/fixtures/migration_sha256.txt. \
             Add `{name} {}` at the end of the list",
            file_hash(&name)
        );
    }
}
