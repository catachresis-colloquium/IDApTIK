//! Load a UMS-compiled `idaptik-package/v1`, run it, print the result as JSON.
//!
//! This is the IDApTIK half of universal-modding-studio's
//! `just roundtrip-idaptik`, which calls
//! `cargo run -p idaptik-core --example package-runner -- <package.json>` and
//! gates the printed `RoundTripResult` with `jq`.
//!
//! Exit codes: 0 on success, 2 when the package cannot be read or loaded
//! (including a missing argument), 3 when the loaded package fails to run.
//! The output is a pure function of the package bytes: `RoundTripResult`
//! holds no map with unordered keys, so two runs print identical bytes.

use idaptik_core::{load_package, run_package};

fn main() {
    let path = std::env::args().nth(1).unwrap_or_default();
    let json = std::fs::read_to_string(&path).unwrap_or_else(|e| fail(2, &format!("{path}: {e}")));
    let loaded = load_package(&json).unwrap_or_else(|e| fail(2, &format!("load: {e}")));
    let result = run_package(loaded).unwrap_or_else(|e| fail(3, &format!("run: {e}")));
    let out = serde_json::to_string(&result).unwrap_or_else(|e| fail(3, &format!("encode: {e}")));
    println!("{out}");
}

fn fail(code: i32, message: &str) -> ! {
    eprintln!("package-runner: {message}");
    std::process::exit(code)
}
