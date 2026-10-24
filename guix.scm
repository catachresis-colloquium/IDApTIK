; SPDX-License-Identifier: AGPL-3.0-or-later
;; GNU Guix development environment for IDApTIK.
;;
;; Usage
;; -----
;;   guix shell -D -f guix.scm                   # the environment, in a subshell
;;   guix shell -D -f guix.scm -- just doctor    # ...and run a recipe inside it
;;   guix shell -D -f guix.scm -C --network -- just loopback-check
;;                                               # ...or in a container
;;
;; `-D` (--development) is not optional. This file defines a *package*, and
;; `guix shell -f` without `-D` puts the package itself in the environment,
;; which would try to build it -- and `(source #f)` cannot build. `-D` puts the
;; package's inputs in the environment instead, which is what a dev shell is.
;;
;; Bare `guix shell` (no arguments) also works, and implies `-D -f` when a
;; guix.scm is found in the working directory or a parent -- but only after
;; that directory is listed in ~/.config/guix/shell-authorized-directories.
;; Do NOT "fix" this file to evaluate to a bare list of packages: bare
;; `guix shell` would then give you the *build dependencies* of each package,
;; which for a toolchain would mean Guix's compiler shadowing the pinned one --
;; precisely the failure this file exists to prevent.
;;
;; Division of authority
;; ---------------------
;; Four files, four jobs, and each decision has exactly one owner:
;;
;;   guix.scm             THE ENVIRONMENT   system libraries, C toolchain,
;;                                          podman, and mise itself
;;   mise.toml            THE VERSIONS      every non-Rust tool
;;   rust-toolchain.toml  THE RUST VERSION  compiler, components, targets
;;   scripts/install-*.sh THE BOOTSTRAP     things no package manager provides
;;
;; The load-bearing decision here is that guix.scm provides `mise` and then
;; STOPS. It does not also provide Nickel, just, Zig, Erlang or Elixir, even
;; though Guix has all five. Providing them from both places would give every
;; one of those tools two versions on PATH and no rule for which wins, which is
;; the same class of defect as the bare `rust` this file avoids. `mise install`
;; is the only thing that puts a versioned tool on PATH, and mise.toml is the
;; only thing that says which version.
;;
;; Rust is provided as RUSTUP, never as `rust`. Guix provides no Rust at all
;; here, so a Guix shell cannot shadow the pin in rust-toolchain.toml. rustup
;; itself is NOT a Guix package (checked against packages.guix.gnu.org,
;; 2026-10-25: "Unknown package: rustup"), so it is bootstrapped by
;; scripts/install-rustup.sh, exactly like Idris2 and trufflehog.
;;
;; What is NOT here
;; ----------------
;; * rustup  — not packaged in Guix. scripts/install-rustup.sh, `just install-rustup`.
;; * Idris2  — bootstraps through Chez Scheme + GMP. scripts/install-idris2.sh.
;; * trufflehog — a Go binary used only by the pre-push secret scan, not a build
;;   input. Installed by scripts/install-git-hooks.sh when needed.
;; All three are excused by name, with these reasons, in scripts/toolchain-parity.sh.
;;
;; What is NOT verified
;; --------------------
;; The package names below were chosen by looking each one up on
;; packages.guix.gnu.org. That proves the package EXISTS; it does not prove this
;; expression evaluates. Nobody has run this file against a Guix daemon yet --
;; see docs/dev-notes/toolchain-authority.md §What is not checked.

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
       '(;; --- the version manager -----------------------------------------
         ;; mise owns every non-Rust tool version (see mise.toml). Guix owns
         ;; mise itself, so there is one bootstrap and no second version table.
         "mise"
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
         ;; curl/wget: scripts/install-rustup.sh fetches the rustup installer with
         ;; one of these, and nothing else in a Guix shell provides either.
         "curl"
         "wget"
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
  (version "0.3.0")
  (source #f)
  (build-system gnu-build-system)
  ;; `inputs`, not `native-inputs`: either is included by
  ;; `package-development-inputs` (what `-D` uses), but `inputs` is the
  ;; spelling every guix.scm dev-environment in the wild uses, so it is the
  ;; one whose behaviour has been exercised by other people's repos.
  (inputs idaptik-tools)
  (synopsis "Development environment for IDApTIK")
  (description
   "Provides the environment IDApTIK builds and runs in: a C toolchain, the
system libraries the Bevy frontend links against, Podman, and mise -- the
version manager that owns every non-Rust tool version.  Rust is bootstrapped
separately with scripts/install-rustup.sh so that rust-toolchain.toml stays the
only authority for the compiler version.")
  (home-page "https://github.com/metadatastician/IDApTIK")
  (license agpl3+))
