# Vendored dependencies

`bevy_winit` is copied from the published Bevy 0.20.0 crate. Its source is
unchanged; only its Cargo feature definitions differ:

- `wayland` enables the native Wayland backend without choosing a decoration
  renderer;
- `wayland-csd-adwaita` preserves Bevy's original titled decorations;
- `wayland-csd-adwaita-notitle` selects Winit's title-free decorations and
  avoids the `ab_glyph` → `ttf-parser` dependency path.

IDApTIK selects the title-free variant while retaining X11 support through
Bevy's normal platform defaults. The upstream MIT and Apache-2.0 license files
are preserved beside the vendored source.

Remove this patch after a released Bevy version exposes independent Wayland CSD
features and IDApTIK has upgraded to it.

## Maintenance obligations are gated

The obligations below are executable, not aspirational (issue #105):

- `just vendor-winit-check` compares this copy with the published crate
  (source, licences, manifest — allowing only the documented feature split)
  and asserts the resolved graph stays free of the font-parser path with the
  title-free CSD feature active. It runs in CI (`vendor-parity` job) and must
  be rerun on every Bevy upgrade.
- `just vendor-winit-watch` checks published `bevy_winit` releases for the
  retirement condition; the scheduled `vendor-winit-watch` workflow fails once
  a release makes this patch removable.
- `just vendor-winit-test` keeps the gate itself honest with clean and firing
  fixtures.

Verified 2026-09-20 at v0.19.1, and re-verified 2026-10-25 at v0.20.0: the
published v0.20.0 manifest still defines
`wayland = ["winit/wayland", "winit/wayland-csd-adwaita"]` — the split has not
landed upstream, so the patch remains required. (The 2026-10-16 Bevy 0.20.0
bump landed without the re-vendor; this re-vendor is what keeps the
font-parser-free graph assertion true again.)

