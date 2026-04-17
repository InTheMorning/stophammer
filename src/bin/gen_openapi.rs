//! Dump the stophammer `OpenAPI` document to stdout.
//!
//! Use this to generate a static `openapi.json` for previewing the themed
//! API explorer without running a full stophammer server.
//!
//! Usage:
//!   `gen_openapi [--readonly]`
//!
//! Flags:
//!   `--readonly`  Emit the read-only node spec (omits write/ingest routes).
//!                 Default is the primary (full) spec.

fn main() {
    let readonly = std::env::args().any(|a| a == "--readonly");

    let doc = if readonly {
        stophammer::openapi::readonly_document()
    } else {
        stophammer::openapi::primary_document()
    };

    let json = serde_json::to_string_pretty(&doc).expect("OpenAPI document must serialise");
    println!("{json}");
}
