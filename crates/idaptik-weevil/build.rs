//! Link-time discovery for the Evil Weevil native kernel.
//!
//! The adapter is a thin Rust host over the *generated* C ABI
//! (`include/evil_weevil/ee.h`); it never hand-edits that header and never
//! reimplements the kernel. It does, however, have to link the kernel's
//! compiled artefact (`libevil_weevil.a` / `.so`), which the evil-weevil
//! repository builds with Zig (`just seam-check` produces it under
//! `src/interface/ffi/zig-out/lib`).
//!
//! This script only adds a search path. It deliberately does **not** stub or
//! skip the library when it is absent: an absent toolchain is a failure, not a
//! silent skip (see `AGENTS.md`, "Checks must be able to fail"). The linker
//! then reports the missing archive by name.
//!
//! Resolution order for the library directory:
//!   1. `EVIL_WEEVIL_LIB_DIR` — an absolute or relative path to the directory
//!      holding `libevil_weevil.*`.
//!   2. `EVIL_WEEVIL_ROOT` — the evil-weevil checkout root; the conventional
//!      Zig output directory is appended.
//!   3. `../../evil-weevil/src/interface/ffi/zig-out/lib` relative to this
//!      crate — the layout used when evil-weevil is cloned beside IDApTIK.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=EVIL_WEEVIL_LIB_DIR");
    println!("cargo:rerun-if-env-changed=EVIL_WEEVIL_ROOT");

    let dir = env::var("EVIL_WEEVIL_LIB_DIR")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            env::var("EVIL_WEEVIL_ROOT").ok().map(|root| {
                let mut p = PathBuf::from(root);
                p.push("src/interface/ffi/zig-out/lib");
                p
            })
        })
        .unwrap_or_else(|| PathBuf::from("../../evil-weevil/src/interface/ffi/zig-out/lib"));

    println!("cargo:rustc-link-search=native={}", dir.display());
    // Echo the resolved path so a failure is legible in the build log.
    println!(
        "cargo:warning=idaptik-weevil: kernel search path = {}",
        dir.display()
    );
}
