# IDApTIK brand tokens: the single source of truth for every colour, typeface,
# type size and spacing value the frontend draws with.
#
# Every value here was lifted verbatim from the code that already draws the
# game (`crates/idaptik-bevy/src/palette.rs`, `fonts.rs`, `hud.rs`, `deck.rs`,
# `sprites.rs`, `scene.rs`) on 2026-10-27; nothing was invented. The usage
# lines are the palette's own doc comments where it had them, and otherwise
# say where the code uses the value. The brand book
# (`docs/design/brand/README.md`) explains the rules; `docs/design/brand/
# inventory.md` lists what is still missing.
#
# Pipeline (see `just brand-check` / `just brand-regen`):
#   brand-tokens.ncl --nickel export--> crates/idaptik-brand/tokens.json (I-JSON,
#   JCS bytes) --cargo test--> crates/idaptik-brand/src/generated.rs (u32
#   0xRRGGBB constants and pixel constants) --> idaptik-bevy::palette.
#
# Check it with `just config-check` (or `nickel export config/brand-tokens.ncl`).
let schema = import "brand-schema.ncl" in

# (Nickel records are recursive, so the parameters cannot share the field
# names.)
let col = fun n v u g s =>
  {
    name = n,
    value = v,
    usage = u,
    group = g,
    seat = s,
    themes = { night = v },
  }
in

let px = fun n v u => { name = n, value = v, usage = u } in

{
  schema_version = "1",

  colour = {
    tokens = [
      # --- Ground: a cold, minimal ground of off-black, concrete grey and off-white.
      col "pitch" "#0c0c0e" "Base night; interiors unlit. Off-black, not blue. The window clear colour." 'ground 'none,
      col "ink2" "#141416" "Second ink: the floor tile's seam line and the building's shell." 'ground 'none,
      col "slate" "#26262a" "Concrete, road, structure." 'ground 'none,
      col "slab" "#36363b" "Door frames, props, the roof slab, the overlay panel's border." 'ground 'none,
      col "dust" "#8e8e94" "Key hints, the isolation meter, edges and trims on concrete; dead view cones." 'ground 'none,
      col "bone" "#f2f0ea" "Paper, type, fiche. The HUD's text colour." 'ground 'none,

      # --- Accents: a few hard flat accents, never soft, never many at once.
      col "cobalt" "#2f5bd6" "Cobalt: civic, open, safe passage. The bandwidth meter and cable trays." 'accent 'none,
      col "phosphor" "#5cf2b4" "The hacker's seat. Screens only: wiring labels and anything wireable." 'accent 'hacker,
      col "signal" "#ffd21f" "Signal yellow: the infiltrator's seat, trim, and anything theirs." 'accent 'infiltrator,
      col "neon" "#e9f1ff" "Cold fluorescent light: room light, windows, street lamps. The trace meter and the end-of-run overlay text." 'accent 'none,
      col "alarm" "#e8202a" "Signal red: threat, alarm, the Machine. Camera LEDs and the crisis wash." 'accent 'threat,

      # --- Scene: the floor's own surfaces, derived from the ground.
      col "room_fill" "#18181b" "Room interior, lights off." 'scene 'none,
      col "room_fill_lit" "#202024" "Room interior, lights on." 'scene 'none,
      col "room_stroke" "#4a4a50" "The room's stroke." 'scene 'none,
      col "sky_top" "#0f0f13" "Sky gradient, top." 'scene 'none,
      col "sky_low" "#1e1e25" "Sky gradient, horizon." 'scene 'none,
      col "skyline" "#0a0a0c" "The skyline silhouette." 'scene 'none,
      col "rain" "#6b6b78" "Rain streaks." 'scene 'none,
      col "grade" "#48484f" "Ground grade line." 'scene 'none,
      col "door_locked" "#5a1418" "Door slab fill when locked by a wire (the bible's oxblood)." 'scene 'none,
      col "door_unlocked" "#1c2a5c" "Door slab fill when unlocked (the bible's verdigris, in this palette's cobalt family)." 'scene 'none,
    ],
  },

  # The three bundled pixel faces (crates/idaptik-bevy/assets/fonts/), all SIL
  # Open Font License 1.1 with the licence text alongside. Every text is drawn
  # without anti-aliasing at a whole multiple of the face's grid.
  font = {
    tokens = [
      {
        name = "vt323",
        value = "VT323",
        usage = "The default UI face: a terminal face on a 16 px grid. Every HUD line, log and screen label.",
        file = "VT323-Regular.ttf",
        licence = "OFL-1.1",
        licence_file = "OFL-vt323.txt",
        grid_px = 16,
        default = true,
      },
      {
        name = "silkscreen",
        value = "Silkscreen",
        usage = "Blocky capitals on an 8 px grid; selectable with IDAPTIK_UI_FONT=silkscreen.",
        file = "Silkscreen-Regular.ttf",
        licence = "OFL-1.1",
        licence_file = "OFL-silkscreen.txt",
        grid_px = 8,
      },
      {
        name = "pixelify",
        value = "Pixelify Sans",
        usage = "A readable pixel sans on an 8 px grid; selectable with IDAPTIK_UI_FONT=pixelify.",
        file = "PixelifySans.ttf",
        licence = "OFL-1.1",
        licence_file = "OFL-pixelifysans.txt",
        grid_px = 8,
      },
    ],
  },

  # The type ramp as the code requests it, in px. At draw time each size is
  # snapped to the chosen face's grid (fonts::UiFont::snap), so the rendered
  # ramp per face is coarser; the brand book tabulates it.
  size = {
    tokens = [
      px "px10" 10 "Wire port labels on the hacker's screens; console sign text.",
      px "px11" 11 "Key hints under the HUD; device labels in the net view.",
      px "px12" 12 "Body copy: the wiring help line, the net HUD, the role-select body.",
      px "px14" 14 "The netplay status line.",
      px "px16" 16 "Deck cards, CRT console text, role-select labels.",
      px "px18" 18 "The role-select subtitle.",
      px "px22" 22 "The end-of-run overlay.",
      px "px24" 24 "The role-select title.",
    ],
  },

  # Spacing as the HUD and deck lay it out (Bevy Val::Px), in px.
  space = {
    tokens = [
      px "hairline" 1 "Deck card border.",
      px "rule" 2 "Overlay panel border; the tight row gap inside a deck card.",
      px "gap_4" 4 "HUD row gap.",
      px "gap_6" 6 "HUD column gap; deck card row gap.",
      px "inset_8" 8 "HUD top inset; deck card padding.",
      px "inset_10" 10 "HUD side inset; deck panel padding; meter bar height.",
      px "pad_16" 16 "Overlay panel vertical padding.",
      px "gap_18" 18 "Deck panel column gap.",
      px "pad_28" 28 "Overlay panel horizontal padding.",
      px "log_bottom" 46 "The log tail's offset from the bottom edge.",
      px "meter_label" 70 "Meter label column width.",
      px "meter_bar" 180 "Meter bar width.",
    ],
  },

  # Pixel art has no rounded corners: Bevy's BorderRadius is never set.
  radius = {
    tokens = [
      px "corner" 0 "Every corner is square; no rounded UI anywhere.",
    ],
  },

  # The art grid (sprites/README.md, scene::PIXEL_SCALE, sprites::SPRITE_PX).
  grid = {
    tokens = [
      px "screen_px_per_art_px" 3 "The floor camera runs at three screen pixels per art pixel.",
      px "figure_art_px" 48 "The infiltrator is drawn 48 art pixels tall.",
      px "figure_units" 54 "The infiltrator is 54 canvas units tall (48 / 54 art px per unit).",
    ],
  },
} | schema.Brand
