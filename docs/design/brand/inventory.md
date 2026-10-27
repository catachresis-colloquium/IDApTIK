<!-- SPDX-FileCopyrightText: 2026 IDApTIK contributors -->
<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# Brand inventory

What the brand handoff (`IDAPTIK_BRAND_HANDOFF.md`, §3) asked for, item by
item, and where each one stands on 2026-10-27. **Done** means committed and
gated; **TODO** names the reason and what would close it. Nothing here was
invented: every committed value came from code that already drew the game,
and every missing source is listed rather than reconstructed.

## Inputs the handoff said must be supplied

| # | Input | Status |
|---|---|---|
| 1 | Source design material (Gemini chat / claude.ai chat / Canva export) | **Not supplied.** Per the handoff's own fallback, the direction was derived from what the repository already draws: `crates/idaptik-bevy/src/palette.rs` (now the token source), `fonts.rs`, and the sprite/backdrop PNGs. The claude.ai Design System artifacts were confirmed empty by the handoff itself. |
| 2 | Gemini's deliverables list | **Not supplied.** This inventory follows the handoff's §3 layout instead. When the list arrives, add one row per item here. |
| 3 | Target repo and branch | This repository, branch `brand/design-system`, draft PR, not merged. |
| 4 | Write access | Granted for this session. |
| 5 | Font, logo and sprite files | Fonts: three OFL faces already bundled (below). Sprites and backdrops: already bundled under `crates/idaptik-bevy/assets/`. Logo: **none exists** (below). |
| 6 | Chronicles of Slavia | Out of scope for this repository; nothing here is Slavia's. |

## Section 3 of the handoff, item by item

The handoff's layout assumed TypeScript + PixiJS. This repository is Rust +
Bevy with no JavaScript runtime (`AGENTS.md`), so the pipeline is the Rust
equivalent; the *shape* of the data is the one the handoff asked for.

| Handoff item | Here | Status |
|---|---|---|
| `design/README.md` brand book | `docs/design/brand/README.md` | **Done.** Real examples; every rule names a token. |
| `design/inventory.md` | this file | **Done.** |
| `design/tokens/tokens.json` single source of truth, list-shaped families with `themes` | Source: `config/brand-tokens.ncl` (typed by `config/brand-schema.ncl`). Export: `crates/idaptik-brand/tokens.json`, list-shaped `{"tokens":[{"name","value","usage",…}]}` per family, colour with `themes`. | **Done.** Gated by `just brand-check` (Nickel typecheck, three negative fixtures, canonical-bytes drift test). |
| `design/palettes/*.gpl` pixel-art palettes (8/16-bit variants) | — | **TODO.** The sprites README names a "shared 38-colour palette" but no palette file is committed. Measured on 2026-10-27 over the embedded PNGs: 36 distinct opaque colours across the 57 sprites, 30 across the 29 backdrop pieces; both sets contain the UI palette's `pitch`, `slate`, `slab`, `room_stroke`, `cobalt`, `alarm` and `neon` verbatim. Close by committing the measured set as `.gpl` **with a Bevy-side test over the embedded PNGs that fails when a sprite uses a colour outside it**; an ungated palette file would rot. |
| `assets/brand/logo/*.svg + *.png` | — | **TODO.** No logo file exists anywhere in the repository or in the supplied material. Not redrawn, per handoff rule 1. The window title is plain text (`main.rs`). |
| `assets/brand/sprites/`, `ui/` (9-slice frames, icons, cursors) | `crates/idaptik-bevy/assets/sprites/` (57 PNGs) and `assets/backdrop/` (29 PNGs), credited in `assets/CREDITS.md` | **Partly done.** Characters, props and backdrops exist. **TODO:** 9-slice UI frames, icons and cursors: none exist; the HUD and deck are drawn with flat `Node`s and 1–2 px borders (see the `space` tokens). |
| `assets/brand/fonts/` + LICENSE | `crates/idaptik-bevy/assets/fonts/`: Silkscreen, VT323, Pixelify Sans, each with its OFL-1.1 text beside it | **Done** (pre-existing). Tokens: `font.*`. |
| `src/theme/tokens.ts` (0xRRGGBB numbers) | `crates/idaptik-brand/src/generated.rs`: `u32` `0xRRGGBB` constants plus pixel constants, generated from `tokens.json` | **Done.** Declared generated in `.gitattributes`; `generated_rs_matches_tokens_json` fails on drift. |
| `src/theme/index.ts` (text styles / palette helpers) | `crates/idaptik-bevy/src/palette.rs` (the `Color` view) and `fonts.rs` (face, grid snapping) | **Done.** `palette.rs` no longer carries values of its own. |
| `scripts/build-tokens.*` (tokens.json → tokens.ts + tokens.css) | `just brand-regen` / `just brand-check` | **Done** for the Rust side. **TODO:** `tokens.css` is not produced: this repository ships no web page that would consume it (the landing-page outline in `docs/design/` is a standalone HTML reference with its own, different palette, see below). Add a CSS renderer beside `render_generated` when a web surface exists. |
| Demo scene rendering palette and type ramp | `crates/idaptik-bevy/examples/brand_sheet.rs`, `just brand-sheet` | **Done.** Renders every colour token and the ramp in the chosen face; `--screenshot` writes the sheets in this directory. |
| PR with screenshots | branch `brand/design-system` | See the pull request. |
| Re-sync the claude.ai Design System artifact | — | **Optional, TODO.** `tokens.json` has the list-shaped families the handoff specified for re-import; nothing was pushed to claude.ai from this session. |

## Sources the code refers to that are not in the repository

| Referent | Where it is cited | Status |
|---|---|---|
| The **storyboard bible** (§5.3 props, §5.4 environment layers, §7.0 posture, §7.3 door colours, §12.1 the hacker's knowledge) | `art.rs`, `scene.rs`, `console.rs` | **TODO: commit or link it.** The code was written against it; the brand book can only quote what the code kept. The deed's roadmap item "story-bible skeleton" is still open. |
| **Oswald / IBM Plex Mono / Inter**, the bible's typefaces | `art.rs` module docs ("the one gap") | **TODO: decide.** The game ships three pixel faces instead and the brand book treats those as the brand. If the bible's faces are wanted for marketing or web surfaces, they are OFL (Oswald, Plex, Inter) and can be bundled with licences; nothing was added without the decision. |
| **"Post-wall industrial, 8/16-bit, Neue Deutsche Welle"** (the stated direction) | the handoff §2 | **Recorded, not re-derived.** The palette's own statement of direction is "early-1980s new wave: a cold, minimal ground of off-black, concrete grey and off-white, and a few hard flat accents"; the brand book carries both and notes where they agree. No Gemini material arrived to add to it. |

## Divergences found while deriving the tokens (flagged, not fixed)

Handoff rule 1 and rule 3: values are taken exactly and failures are flagged.
None of the following was changed on this branch.

| Finding | Where | Suggested closing move |
|---|---|---|
| **Alarm red fails 4.5:1 as text on night** (4.35:1). It passes 3:1, so it is fine as a glyph, LED or wash, and the HUD never draws it as text today. | `legibility.rs` pins the verdict | Keep it off body copy, or adopt a lighter text-red token if one is ever needed. |
| **Dust on slab is 3.69:1**: legible as a glyph, not as text. | `legibility.rs` | Never set dust type on a slab surface; use bone. |
| **Cobalt on night is 3.34:1**: a bar colour, not a text colour. | `legibility.rs` | As above. |
| **The type ramp collapses in the default face.** VT323 snaps to a 16 px grid, so seven of the eight requested sizes (10–22 px) all draw at 16 px and only 24 px steps up to 32 px. On the 8 px faces the ramp is 8 / 16 / 24. | `fonts::UiFont::snap`, tabulated in the brand book | Either request grid-aligned sizes in the code (so what is typed is what is drawn) or accept a two-step ramp as the brand. |
| **Off-palette literals.** `sprites.rs` colours devices by kind with a Material-design set (`#2196f3`, `#ff9800`, `#d32f2f`, `#9c27b0`, `#f44336`, `#4caf50`, `#ffeb3b`, `#795548`, `#757575`, `#fff59d`, `#616161`), pure green LEDs and console text (`0,1,0`), and ad-hoc greys; `net_view.rs` and `netplay/ui/role_select.rs` use their own greys, white and tints; `backdrop.rs` has two tint constants; `scene.rs` has three door colours (`DOOR_SHUT`, `DOOR_ROUTING`, `DOOR_OPEN`) beside the two palette ones. | those files | Migrate each to a token (new tokens where a role is real, e.g. a device-kind family), then add a test that `idaptik-bevy` names no `Color::srgb*` literal outside `palette.rs`. Not done here: it changes what the game draws, which is a design decision, not a token extraction. |
| **The landing-page outline uses a different palette** (teal/cyan ground `#071014`, `--cyan #58e0da`, `--amber #e5b95d`, Inter and a mono stack). | `docs/design/idaptik-landing-page.html` | Decide whether the web surface follows the game palette (then `tokens.css` closes it) or stays its own; today they do not match. |
| **The sprites' "38-colour palette" is 36 (sprites) and 30 (backdrops) as measured.** | `assets/sprites/README.md` | Commit the measured palette with a gate (above) and correct the README from the gate's count. |

## What is deliberately not here

- No TypeScript, PixiJS, npm or CSS build: forbidden by `AGENTS.md`, and there is no consumer.
- No new colour, no renamed token, no redrawn asset: handoff rule 1.
- No Chronicles of Slavia material.
- No claim that the kyq gate canonicalised `tokens.json`: kyq is not yet deployable (`docs/standards-conformance.md`); `crates/idaptik-brand/src/canonical.rs` is the transitional canonicaliser for this one file and says so.
