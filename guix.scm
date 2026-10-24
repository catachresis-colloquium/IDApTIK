; SPDX-License-Identifier: AGPL-3.0-or-later
;; GNU Guix development environment for IDApTIK.
;;
;; Usage
;; -----
;;   guix shell -f guix.scm                      # the environment, in a subshell
;;   guix shell -f guix.scm -- just doctor       # ...and run a recipe inside it
;;   guix shell -f guix.scm -C --network -- just loopback-check
;;                                               # ...or in a container
;;
;; Division of authority
;; ---------------------
;; Three files, three jobs, and each version has exactly one authority:
;;
;;   guix.scm             THE ENVIRONMENT — system libraries, C toolchain, the
;;                        tools themselves, and the container runtime
;;   mise.toml            THE VERSIONS of the non-Rust tools
;;   rust-toolchain.toml  THE VERSION of Rust
;;
;; This file is the environment policy (nix is retired; Guix is the policy).
;; It is deliberately NOT a second version pin. Guix's nickel, zig and just are
;; whatever the Guix revision in use provides; mise's copies are what `just`
;; recipes invoke, and `scripts/toolchain-parity.sh` reports any divergence
;; instead of letting two files quietly disagree again.
;;
;; Rust is provided as RUSTUP, never as `rust`. That is the whole trick: it
;; means `rust-toolchain.toml` remains the single authority for the compiler
;; version, in a Guix shell exactly as on a mise machine. Providing Guix's
;; `rust` here would create a second, silently older compiler.
;;
;; What is NOT here
;; ----------------
;; * Idris2 — provisioned on demand by `scripts/install-idris2.sh`, because it
;;   bootstraps through Chez Scheme + GMP. `just install-idris2`.
;; * trufflehog — a Go binary used only by the pre-push secret scan, not a build
;;   input. Installed by `scripts/install-git-hooks.sh` when needed.
;; Both are excused by name, with these reasons, in scripts/toolchain-parity.sh.

(use-modules ((gnu packages) #:select (specification->package))
             (guix packages)
             (guix build-system gnu)
             (guix licenses))

;; Resolve by name rather than by module import: the names below are stable
;; across Guix revisions, the module paths are not, and a missing module is a
;; worse error than a missing package.
(define (pkg name)
  (specification->package name))

(define idaptik-tools
  (map pkg
       '(;; --- version-managed toolchains -----------------------------------
         ;; Rust comes as rustup so rust-toolchain.toml stays authoritative.
         "rustup"
         "nickel"
         "just"
         "zig"
         "elixir"
         "erlang"
         ;; --- container runtime -------------------------------------------
         ;; Podman, not Docker: rootless by default, and the estate's
         ;; .devcontainer/ is runtime-agnostic (see docs/dev-notes/
         ;; toolchain-authority.md §Containers).
         "podman"
         ;; --- build essentials --------------------------------------------
         "gcc-toolchain"
         "clang"
         "lld"
         "cmake"
         "pkg-config"
         "git"
         "openssl"
         "coreutils"
         "findutils"
         "grep"
         "sed"
         "bash"
         ;; --- system libraries the Bevy build links against ----------------
         ;; `just bevy-linux-deps` installs the same set via apt on Debian;
         ;; this list is the Guix equivalent and must be kept in step by hand.
         "alsa-lib"
         "wayland"
         "libxkbcommon"
         "vulkan-loader"
         "mesa"
         "libx11"
         "libxcursor"
         "libxi"
         "libxrandr"
         "libxinerama"
         "libice"
         "libsm"
         ;; --- fonts. Open Sans is Bevy's default face; the TUI needs a
         ;; monospace fallback with box-drawing glyphs.
         "fontconfig"
         "font-dejavu")))

(package
  (name "idaptik-env")
  (version "0.2.0")
  (source #f)
  (build-system gnu-build-system)
  (native-inputs idaptik-tools)
  (synopsis "Development environment for IDApTIK")
  (description
   "Provides the environment IDApTIK builds and runs in: the Rust toolchain
(through rustup, so rust-toolchain.toml stays authoritative), Nickel, just, Zig,
Elixir, Erlang, Podman, a C toolchain, and the system libraries the Bevy
frontend links against.  Tool versions are pinned by mise.toml and
rust-toolchain.toml; this package provides the environment, not the pins.")
  (home-page "https://github.com/metadatastician/IDApTIK")
  (license agpl3+))
