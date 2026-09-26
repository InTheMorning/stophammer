fn main() {
    println!("cargo:rerun-if-env-changed=STOPHAMMER_GIT_REVISION");
    println!("cargo:rerun-if-env-changed=STOPHAMMER_BUILT_AT");
}
