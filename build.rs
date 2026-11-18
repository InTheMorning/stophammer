//! Gives the git revision and the build time to the compiler
//! (musicindex ADR 0083 R-VERSION-8). The inputs are `STOPHAMMER_GIT_REVISION`
//! and `STOPHAMMER_BUILT_AT`, as `deploy.sh` and the `Dockerfile` set them.

fn main() {
    musicindex_build_info::build::emit("STOPHAMMER");
}
