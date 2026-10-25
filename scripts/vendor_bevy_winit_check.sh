#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Gate for the vendored bevy_winit feature patch (issue #105).
#
# PR #104 vendored the published bevy_winit source so IDApTIK can select
# Winit's title-free Wayland CSD feature without the ab_glyph -> ttf-parser
# dependency path that Bevy's `wayland` feature otherwise forces. A vendored
# crate receives no upstream fixes, so this gate makes the maintenance
# obligation in vendor/README.md executable:
#
#   parity — the vendored copy is byte-identical to the published crate
#            (source, licences, README) and its manifest differs ONLY by the
#            documented feature split. Run this on every Bevy upgrade.
#   graph  — the resolved workspace graph contains no font-parser crates and
#            activates the title-free CSD feature on bevy_winit/winit.
#   watch  — inspect published bevy_winit releases for the retirement
#            condition (independently selectable Wayland CSD features).
#            Exits non-zero once upstream makes the vendor patch removable,
#            so the patch cannot silently outlive its welcome.
#
# Offline fixtures: set IDAPTIK_BEVY_WINIT_CRATE to a local .crate tarball
# (parity), IDAPTIK_BEVY_WINIT_INDEX to a local sparse-index JSONL file
# (watch), and/or IDAPTIK_BEVY_WINIT_METADATA to a local `cargo metadata`
# JSON document (graph). All default to the live sources.

set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VENDOR_DIR="$REPO_DIR/vendor/bevy_winit"
CRATE_NAME="bevy_winit"
CRATES_BASE="https://static.crates.io/crates/$CRATE_NAME"
SPARSE_INDEX="https://index.crates.io/be/vy/$CRATE_NAME"

# The three feature definitions the vendored patch owns. Everything else in
# the vendored tree must match the published crate exactly.
VENDORED_WAYLAND='wayland = ["winit/wayland"]'
VENDORED_ADWAITA='wayland-csd-adwaita = ["wayland", "winit/wayland-csd-adwaita"]'
VENDORED_NOTITLE='wayland-csd-adwaita-notitle = ["wayland", "winit/wayland-csd-adwaita-notitle"]'
# What the published manifest defines instead (single forced feature).
PUBLISHED_WAYLAND='wayland = ["winit/wayland", "winit/wayland-csd-adwaita"]'

say()  { printf '[vendor-winit] %s\n' "$*"; }
# Report all arguments as one failure message, emit a GitHub Actions error
# annotation on stdout, and exit with status 1.
die()  {
  printf '[vendor-winit] FAIL: %s\n' "$*" >&2
  # Also emit a check-run annotation: without it the failure detail exists
  # only in the run log, which a remote operator (or a sandbox without log
  # egress) cannot read -- a gate that cannot be diagnosed from outside
  # cannot be cleared from outside. Flatten multiline diagnostics for the
  # annotation while retaining the original formatting on stderr.
  local annotation
  annotation="$(printf '%s' "$*" | tr '\n' ' ' | sed 's/[[:space:]][[:space:]]*/ /g')"
  printf '::error::[vendor-winit] %s\n' "$annotation"
  exit 1
}

# Temp dirs/files are cleaned on exit — including the failure paths, where the
# diagnostic has already been printed.
CLEANUP_PATHS=()
cleanup() { [ ${#CLEANUP_PATHS[@]} -eq 0 ] || rm -rf -- "${CLEANUP_PATHS[@]}"; }
trap cleanup EXIT

need() { command -v "$1" >/dev/null 2>&1 || die "$1 is required for this check ($2)"; }

vendored_version() {
  awk -F'"' '/^version = /{print $2; exit}' "$VENDOR_DIR/Cargo.toml"
}

# Strip one feature definition (single- or multi-line) from a cargo-normalized
# manifest on stdin and print the rest verbatim. $1 = feature name.
strip_feature() {
  awk -v feature="$1" '
    $0 ~ "^"feature" = " { in_feature = 1 }
    in_feature {
      if ($0 ~ /\][,]?[[:space:]]*$/) { in_feature = 0 }
      next
    }
    { print }
  '
}

# Print `name<TAB>sorted,comma,items` for every feature in the [features]
# section of a cargo-normalized manifest on stdin. Handles single- and
# multi-line feature definitions.
extract_features() {
  awk '
    function emit(name, line,   items, n, arr, i, j, t) {
      items = ""
      while (match(line, /"[^"]*"/)) {
        item = substr(line, RSTART + 1, RLENGTH - 2)
        items = items (items == "" ? "" : "\n") item
        line = substr(line, RSTART + RLENGTH)
      }
      n = split(items, arr, "\n")
      for (i = 1; i < n; i++) for (j = i + 1; j <= n; j++)
        if (arr[j] < arr[i]) { t = arr[i]; arr[i] = arr[j]; arr[j] = t }
      printf "%s\t", name
      for (i = 1; i <= n; i++) printf "%s%s", arr[i], (i < n ? "," : "")
      printf "\n"
    }
    /^\[features\]/ { in_features = 1; next }
    /^\[/ { in_features = 0; collecting = 0 }
    in_features && !collecting && /^[a-zA-Z0-9_-]+ = / {
      name = $1
      line = $0
      sub(/^[a-zA-Z0-9_-]+ = /, "", line)
      if (line ~ /\][,]?[[:space:]]*$/) {
        emit(name, line)
      } else {
        buf = line
        collecting = 1
      }
      next
    }
    collecting {
      buf = buf " " $0
      if ($0 ~ /\][,]?[[:space:]]*$/) {
        collecting = 0
        emit(name, buf)
      }
      next
    }
  '
}

feature_value() { # $1 = features output, $2 = feature name
  awk -F'\t' -v f="$2" '$1 == f { print $2 }' <<<"$1"
}

# ---------------------------------------------------------------- parity ----

check_parity() {
  need tar "unpacking the published crate"

  local version
  version="$(vendored_version)"
  [ -n "$version" ] || die "cannot read the vendored bevy_winit version"

  # The workspace must actually route bevy_winit through the vendored copy.
  grep -q '^\[patch.crates-io\]' "$REPO_DIR/Cargo.toml" ||
    die "root Cargo.toml has no [patch.crates-io] section; is the vendor patch unwired?"
  grep -q "^bevy_winit = { path = \"vendor/bevy_winit\" }" "$REPO_DIR/Cargo.toml" ||
    die "root Cargo.toml no longer patches bevy_winit to vendor/bevy_winit"

  local crate_file work
  work="$(mktemp -d)"
  CLEANUP_PATHS+=("$work")

  if [ -n "${IDAPTIK_BEVY_WINIT_CRATE:-}" ]; then
    crate_file="$IDAPTIK_BEVY_WINIT_CRATE"
    say "using local crate fixture: $crate_file"
  else
    need curl "downloading the published crate"
    crate_file="$work/$CRATE_NAME-$version.crate"
    say "downloading $CRATE_NAME-$version from crates.io…"
    curl -fsSL --retry 3 -o "$crate_file" "$CRATES_BASE/$CRATE_NAME-$version.crate" ||
      die "could not download the published $CRATE_NAME-$version crate"
  fi

  mkdir -p "$work/published"
  tar -xzf "$crate_file" -C "$work/published" ||
    die "could not unpack the published crate tarball"
  local published="$work/published/$CRATE_NAME-$version"
  [ -d "$published" ] || die "tarball did not contain $CRATE_NAME-$version/"

  # 1. Source, licences, and README are byte-identical.
  #
  # `cargo package` GENERATES three files into every tarball. They are correctly
  # absent from a vendored source tree, so their absence is not drift -- before
  # this was handled the gate failed on every real crates.io artefact while
  # passing its whole fixture suite (issue #123):
  #   .cargo_vcs_info.json  the upstream commit the tarball was cut from
  #   Cargo.lock            the crate's own lock, meaningless to a vendored dep
  #   Cargo.toml.orig       cargo's copy of the pre-normalisation manifest
  #
  # Removed at the crate ROOT rather than passed to `diff --exclude`, because
  # --exclude matches a BASENAME AT ANY DEPTH: `--exclude=Cargo.lock` would also
  # blind the check to a real `src/Cargo.lock`. Measured -- that exact mutant
  # survived the --exclude form and is killed by this one. cargo only ever emits
  # these three at the root, so removing them there is exact.
  #
  # $published is our own `mktemp -d` unpack (see $work above), never the
  # vendored tree, and nothing below reads these files -- only Cargo.toml, which
  # is deliberately kept and compared properly by section 2.
  rm -f -- \
    "$published/.cargo_vcs_info.json" \
    "$published/Cargo.lock" \
    "$published/Cargo.toml.orig"

  local drift
  drift="$(diff -rq "$published" "$VENDOR_DIR" --exclude=Cargo.toml 2>&1 || true)"
  [ -z "$drift" ] || die "vendored tree drifted from the published crate:
$drift
Re-vendor from the published $CRATE_NAME-$version crate (vendor/README.md)."

  # 2. The manifests differ ONLY by the documented feature split.
  local vendored_rest published_rest
  vendored_rest="$(strip_feature wayland < "$VENDOR_DIR/Cargo.toml" \
    | strip_feature wayland-csd-adwaita | strip_feature wayland-csd-adwaita-notitle)"
  published_rest="$(strip_feature wayland < "$published/Cargo.toml")"
  [ "$vendored_rest" = "$published_rest" ] ||
    die "vendored Cargo.toml differs from the published crate beyond the documented feature split:
$(diff -u <(printf '%s\n' "$published_rest") <(printf '%s\n' "$vendored_rest") || true)"

  # 3. The feature split is exactly the documented one, in both manifests.
  local vendored_features published_features
  vendored_features="$(extract_features < "$VENDOR_DIR/Cargo.toml")"
  published_features="$(extract_features < "$published/Cargo.toml")"

  local expected_vendored_wayland expected_published_wayland
  expected_vendored_wayland="$(feature_value "$vendored_features" wayland)"
  expected_published_wayland="$(feature_value "$published_features" wayland)"

  [ "$expected_vendored_wayland" = "winit/wayland" ] ||
    die "vendored feature 'wayland' must be exactly [\"winit/wayland\"], got [$expected_vendored_wayland]"
  [ "$expected_published_wayland" = "winit/wayland,winit/wayland-csd-adwaita" ] ||
    die "published feature 'wayland' changed upstream (got [$expected_published_wayland]); \
the vendor patch premise needs review"
  [ "$(feature_value "$vendored_features" wayland-csd-adwaita)" = "wayland,winit/wayland-csd-adwaita" ] ||
    die "vendored feature 'wayland-csd-adwaita' must preserve Bevy's original titled decorations"
  [ "$(feature_value "$vendored_features" wayland-csd-adwaita-notitle)" = "wayland,winit/wayland-csd-adwaita-notitle" ] ||
    die "vendored feature 'wayland-csd-adwaita-notitle' must select Winit's title-free decorations"

  # 4. No extra or missing features beyond the split.
  local vendored_count published_count
  vendored_count="$(wc -l <<<"$vendored_features")"
  published_count="$(wc -l <<<"$published_features")"
  [ "$vendored_count" -eq $((published_count + 2)) ] ||
    die "vendored manifest has $vendored_count features; expected the published $published_count plus the two split features"

  local published_version
  published_version="$(awk -F'"' '/^version = /{print $2; exit}' "$published/Cargo.toml")"
  [ "$published_version" = "$version" ] ||
    die "published crate version $published_version does not match the vendored $version"

  say "parity: vendored $CRATE_NAME-$version matches the published crate except the documented feature split."
}

# ----------------------------------------------------------------- graph ----

# Validate font-parser exclusion and title-free Wayland CSD selection using
# IDAPTIK_BEVY_WINIT_METADATA when set, otherwise locked Cargo metadata.
# Requires cargo and jq even with a fixture; exits on missing tools, metadata
# errors, or graph violations.
check_graph() {
  need cargo "resolving the workspace dependency graph"
  need jq "inspecting cargo metadata"

  local metadata
  if [ -n "${IDAPTIK_BEVY_WINIT_METADATA:-}" ]; then
    # Fixture injection, mirroring IDAPTIK_BEVY_WINIT_CRATE/_INDEX above. The
    # graph assertions are pure functions of this document, so feeding a
    # crafted one is the only way to exercise them: mutating Cargo.toml
    # instead invalidates Cargo.lock and `--locked` then fails FIRST, so the
    # gate fires for the wrong reason and proves nothing about these checks.
    metadata="$(cat "$IDAPTIK_BEVY_WINIT_METADATA")"
  else
    local work metadata_file metadata_err detail
    work="$(mktemp -d)"
    CLEANUP_PATHS+=("$work")
    metadata_file="$work/cargo-metadata.json"
    metadata_err="$work/cargo-metadata.err"
    if cargo metadata --format-version 1 --locked >"$metadata_file" 2>"$metadata_err"; then
      metadata="$(cat "$metadata_file")"
    else
      detail="$(tail -n 20 "$metadata_err" | tr '\n' ' ' | sed 's/[[:space:]][[:space:]]*/ /g')"
      die "cargo metadata --locked failed; is the workspace resolvable? $detail"
    fi
  fi

  # 1. The font-parser path must not return to the resolved graph.
  local offenders
  offenders="$(jq -r '[.packages[].name] | map(select(. == "ttf-parser" or . == "ab_glyph" or . == "ab_glyph_rasterizer")) | .[]' <<<"$metadata")"
  [ -z "$offenders" ] ||
    die "font-parser crates are back in the resolved graph: $offenders
(The vendored patch exists to keep the ab_glyph -> ttf-parser path out; see issue #105.)"

  # 2. Exactly one bevy_winit (the patched path dependency) is resolved, with
  #    the title-free CSD feature active.
  #
  # Resolve the node by looking its id up in `.packages[]` BY NAME, never by
  # matching the id's text. cargo's package-id format changed: ids are opaque
  # PackageIdSpec URLs now (`path+file:///.../vendor/bevy_winit#0.19.1`, and
  # `...#name@version` when the directory name differs from the crate name),
  # not the legacy `bevy_winit 0.19.1 (path+file://...)`. The old
  # `startswith("bevy_winit ")` therefore matched NOTHING on any modern
  # toolchain: it reported 0 nodes and blamed the [patch.crates-io] wiring,
  # which was never broken. Measured on the pinned cargo 1.95.0.
  #
  # `.packages[].name` is a real field with a stable meaning, so this survives
  # any further change to how ids are spelled.
  local node_count bevy_winit_features
  node_count="$(jq '[.packages[] | select(.name == "bevy_winit") | .id] as $ids
    | [.resolve.nodes[] | select(.id | IN($ids[]))] | length' <<<"$metadata")"
  [ "$node_count" -eq 1 ] ||
    die "expected exactly one resolved bevy_winit node, found $node_count; \
is the [patch.crates-io] wiring broken?"
  bevy_winit_features="$(jq -r '[.packages[] | select(.name == "bevy_winit") | .id] as $ids
    | .resolve.nodes[]
    | select(.id | IN($ids[]))
    | .features[]' <<<"$metadata")"
  grep -qx 'wayland-csd-adwaita-notitle' <<<"$bevy_winit_features" ||
    die "bevy_winit resolves without the 'wayland-csd-adwaita-notitle' feature active; \
the workspace bevy_winit dependency must select it"

  # 3. The titled-CSD feature (the one that pulls the font parser) is OFF.
  local winit_node_features
  #
  # One feature PER LINE, matched with `grep -qx`, never `grep -qw` on a
  # space-joined string. `-w` treats `-` as a word boundary, so
  # `grep -qw wayland-csd-adwaita` MATCHES inside `wayland-csd-adwaita-notitle`
  # -- i.e. the titled-CSD check fired precisely when the title-FREE feature
  # was correctly active. Measured. That never surfaced because check 2 above
  # died first on every modern toolchain, so this arm was unreachable.
  # `-qx` anchors the whole line, which is the exact token test intended, and
  # matches how the bevy_winit feature is checked above.
  winit_node_features="$(jq -r '[.packages[] | select(.name == "winit") | .id] as $ids
    | .resolve.nodes[]
    | select(.id | IN($ids[]))
    | .features[]' <<<"$metadata")"
  if grep -qx 'wayland-csd-adwaita' <<<"$winit_node_features"; then
    die "winit resolves with the titled 'wayland-csd-adwaita' feature active; \
the font-parser-free CSD selection has regressed"
  fi
  grep -qx 'wayland-csd-adwaita-notitle' <<<"$winit_node_features" ||
    die "winit resolves without 'wayland-csd-adwaita-notitle'; native Wayland CSD is not wired"

  say "graph: font-parser crates absent; title-free Wayland CSD feature is active."
}

# ----------------------------------------------------------------- watch ----

version_gt() { # $1 > $2 by sort -V semantics
  [ "$1" = "$2" ] && return 1
  local highest
  highest="$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -1)"
  [ "$highest" = "$1" ]
}

check_watch() {
  need jq "inspecting index records"

  local version
  version="$(vendored_version)"
  [ -n "$version" ] || die "cannot read the vendored bevy_winit version"

  local index_file
  if [ -n "${IDAPTIK_BEVY_WINIT_INDEX:-}" ]; then
    index_file="$IDAPTIK_BEVY_WINIT_INDEX"
    say "using local index fixture: $index_file"
  else
    need curl "reading the crates.io sparse index"
    index_file="$(mktemp)"
    CLEANUP_PATHS+=("$index_file")
    say "reading the sparse index for $CRATE_NAME…"
    curl -fsSL --retry 3 -o "$index_file" "$SPARSE_INDEX" ||
      die "could not read $SPARSE_INDEX"
  fi

  # Retirement predicate (vendor/README.md, issue #105): a published release
  # makes the patch removable when EITHER
  #   (a) it ships a feature that selects winit's title-free CSD
  #       (contains "winit/wayland-csd-adwaita-notitle"), or
  #   (b) its `wayland` feature no longer forces "winit/wayland-csd-adwaita".
  local vers yanked wayland_deps notitle_features line retired=""
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    vers="$(jq -r '.vers' <<<"$line")"
    yanked="$(jq -r '.yanked // false' <<<"$line")"
    [ "$yanked" = "false" ] || continue
    version_gt "$vers" "$version" || continue

    wayland_deps="$(jq -r '.features.wayland // [] | join(",")' <<<"$line")"
    notitle_features="$(jq -r '[.features | to_entries[]
      | select((.value | index("winit/wayland-csd-adwaita-notitle")) != null)
      | .key] | join(",")' <<<"$line")"

    if [ -n "$notitle_features" ]; then
      retired="$retired
  $vers: feature '$notitle_features' selects title-free Wayland CSD"
    elif [ -n "$wayland_deps" ] && ! grep -q 'winit/wayland-csd-adwaita' <<<"$wayland_deps"; then
      retired="$retired
  $vers: 'wayland' no longer forces the titled CSD ($wayland_deps)"
    fi
  done < "$index_file"

  if [ -n "$retired" ]; then
    say "RETIREMENT CONDITION MET — remove the vendor patch (issue #105):$retired"
    say "steps: drop [patch.crates-io] and vendor/bevy_winit, select the upstream"
    say "feature in Cargo.toml, rerun just vendor-winit-check and the Bevy CI jobs."
    exit 1
  fi

  say "watch: no published bevy_winit release above $version exposes independently"
  say "selectable Wayland CSD features; the vendored patch is still required."
}

# ------------------------------------------------------------------ main ----

case "${1:-}" in
  parity) check_parity ;;
  graph)  check_graph ;;
  watch)  check_watch ;;
  *)      printf 'usage: %s parity|graph|watch\n' "${0##*/}" >&2; exit 2 ;;
esac
