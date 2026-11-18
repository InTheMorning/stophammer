//! musicindex ADR 0083 R-VERSION-9 item 1: the `--version` output contains
//! the build values of R-VERSION-1. `fg05_community_peers_tests` examines the
//! same values in `GET /node/info`.

#[test]
fn version_flag_prints_the_three_build_values() {
    let build = stophammer::BUILD_INFO;
    let revision = build.git_revision.expect(
        "musicindex ADR 0083 R-VERSION-1: run the test in a Git checkout or set STOPHAMMER_GIT_REVISION",
    );
    let built_at = build
        .built_at
        .expect("musicindex ADR 0083 R-VERSION-1: the test build has no build time");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_stophammer"))
        .arg("--version")
        .output()
        .expect("run stophammer --version");
    assert!(output.status.success(), "stophammer --version must exit 0");

    let line = String::from_utf8(output.stdout).expect("UTF-8 output");
    let expected = format!(
        "stophammer {} ({revision}, {})\n",
        env!("CARGO_PKG_VERSION"),
        &built_at[..10]
    );
    assert_eq!(
        line, expected,
        "musicindex ADR 0083 R-VERSION-2: --version must print BUILD_INFO.version_line()"
    );
}
