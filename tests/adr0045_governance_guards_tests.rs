// ADR 0045 task 001: the ADR archive and the two governance guards.
//
// docs/tasks/adr-0045-task-001-archive-and-guards.md
//
// Guard 1 checks that `AGENTS.md` is tracked by git. A decision and the
// prose that restates it must move in the same commit, and a file outside
// version control cannot move with a commit.
//
// Guard 2 checks that the ADR files of `docs/adr/` and `docs/adr/archive/`
// agree with the rows of `docs/adr/README.md`: each file has a row, and each
// row names a file that exists.
//
// This test runs `git`, not `cargo`, so it does not wait on the build lock
// of the outer `cargo test`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn agents_md_is_tracked_by_git() {
    let output = Command::new("git")
        .args(["ls-files", "AGENTS.md"])
        .current_dir(manifest_dir())
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "ADR 0045 §Guards: could not run `git ls-files AGENTS.md` ({e}). \
                 Install git and put it on PATH before running this test."
            )
        });
    assert!(
        output.status.success(),
        "ADR 0045 §Guards: `git ls-files AGENTS.md` exited with an error. \
         Run this test inside the stophammer git repository."
    );
    let listed = String::from_utf8_lossy(&output.stdout);
    assert!(
        !listed.trim().is_empty(),
        "ADR 0045 §Guards: AGENTS.md is not tracked by git. Track AGENTS.md, \
         so a decision and the prose that restates it move in the same commit."
    );
}

/// True when `target` names an ADR file: an optional `archive/`, four ASCII
/// digits, a dash, then any text ending in `.md`.
fn is_adr_link_target(target: &str) -> bool {
    let name = target.strip_prefix("archive/").unwrap_or(target);
    let bytes = name.as_bytes();
    bytes.len() > 5
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

/// Each markdown link target of `text` that names an ADR file, in the form
/// `NNNN-….md` or `archive/NNNN-….md`.
fn adr_link_targets(text: &str) -> BTreeSet<String> {
    let mut targets = BTreeSet::new();
    let mut rest = text;
    while let Some(pos) = rest.find("](") {
        let start = pos + "](".len();
        let Some(len) = rest[start..].find(')') else {
            break;
        };
        let target = &rest[start..start + len];
        if is_adr_link_target(target) {
            targets.insert(target.to_string());
        }
        rest = &rest[start + len..];
    }
    targets
}

/// Each ADR file name in `dir`, relative to `dir`, except `README.md` and
/// anything under `templates/`.
fn adr_file_names(dir: &Path) -> BTreeSet<String> {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| entry.unwrap_or_else(|e| panic!("read entry of {}: {e}", dir.display())));
    let mut names = BTreeSet::new();
    for entry in entries {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md")
            && path.file_name().is_some_and(|name| name != "README.md")
        {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .expect("an ADR file name is valid UTF-8");
            names.insert(name.to_string());
        }
    }
    names
}

#[test]
fn every_adr_file_has_a_row_and_every_row_names_a_file() {
    let adr_dir = manifest_dir().join("docs/adr");
    let archive_dir = adr_dir.join("archive");

    let mut files: BTreeSet<String> = adr_file_names(&adr_dir);
    for name in adr_file_names(&archive_dir) {
        files.insert(format!("archive/{name}"));
    }
    assert!(
        files.len() >= 60,
        "the test found only {} ADR files under {}; check the directory listing",
        files.len(),
        adr_dir.display()
    );

    let readme_path = adr_dir.join("README.md");
    let readme = std::fs::read_to_string(&readme_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", readme_path.display()));
    let rows = adr_link_targets(&readme);

    for file in &files {
        assert!(
            rows.contains(file),
            "ADR 0045 §Guards: docs/adr/{file} has no row in docs/adr/README.md. \
             Add a row to the index."
        );
    }

    for row in &rows {
        let target = adr_dir.join(row);
        assert!(
            target.is_file(),
            "ADR 0045 §Guards: docs/adr/README.md names docs/adr/{row}, which does \
             not exist. Fix the link, or remove the row."
        );
    }
}
