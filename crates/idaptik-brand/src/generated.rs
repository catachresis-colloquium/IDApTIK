//! GENERATED from `tokens.json` by `just brand-regen` (`Brand::render_generated`).
//! Do not edit: change `config/brand-tokens.ncl` and regenerate. The test
//! `generated_rs_matches_tokens_json` fails if this file and the JSON disagree.

/// Colours as `0xRRGGBB`, in source order.
#[rustfmt::skip]
pub mod colour {
    /// Base night; interiors unlit. Off-black, not blue. The window clear colour. (ground, seat: none)
    pub const PITCH: u32 = 0x0c0c0e;
    /// Second ink: the floor tile's seam line and the building's shell. (ground, seat: none)
    pub const INK2: u32 = 0x141416;
    /// Concrete, road, structure. (ground, seat: none)
    pub const SLATE: u32 = 0x26262a;
    /// Door frames, props, the roof slab, the overlay panel's border. (ground, seat: none)
    pub const SLAB: u32 = 0x36363b;
    /// Key hints, the isolation meter, edges and trims on concrete; dead view cones. (ground, seat: none)
    pub const DUST: u32 = 0x8e8e94;
    /// Paper, type, fiche. The HUD's text colour. (ground, seat: none)
    pub const BONE: u32 = 0xf2f0ea;
    /// Cobalt: civic, open, safe passage. The bandwidth meter and cable trays. (accent, seat: none)
    pub const COBALT: u32 = 0x2f5bd6;
    /// The hacker's seat. Screens only: wiring labels and anything wireable. (accent, seat: hacker)
    pub const PHOSPHOR: u32 = 0x5cf2b4;
    /// Signal yellow: the infiltrator's seat, trim, and anything theirs. (accent, seat: infiltrator)
    pub const SIGNAL: u32 = 0xffd21f;
    /// Cold fluorescent light: room light, windows, street lamps. The trace meter and the end-of-run overlay text. (accent, seat: none)
    pub const NEON: u32 = 0xe9f1ff;
    /// Signal red: threat, alarm, the Machine. Camera LEDs and the crisis wash. (accent, seat: threat)
    pub const ALARM: u32 = 0xe8202a;
    /// Room interior, lights off. (scene, seat: none)
    pub const ROOM_FILL: u32 = 0x18181b;
    /// Room interior, lights on. (scene, seat: none)
    pub const ROOM_FILL_LIT: u32 = 0x202024;
    /// The room's stroke. (scene, seat: none)
    pub const ROOM_STROKE: u32 = 0x4a4a50;
    /// Sky gradient, top. (scene, seat: none)
    pub const SKY_TOP: u32 = 0x0f0f13;
    /// Sky gradient, horizon. (scene, seat: none)
    pub const SKY_LOW: u32 = 0x1e1e25;
    /// The skyline silhouette. (scene, seat: none)
    pub const SKYLINE: u32 = 0x0a0a0c;
    /// Rain streaks. (scene, seat: none)
    pub const RAIN: u32 = 0x6b6b78;
    /// Ground grade line. (scene, seat: none)
    pub const GRADE: u32 = 0x48484f;
    /// Door slab fill when locked by a wire (the bible's oxblood). (scene, seat: none)
    pub const DOOR_LOCKED: u32 = 0x5a1418;
    /// Door slab fill when unlocked (the bible's verdigris, in this palette's cobalt family). (scene, seat: none)
    pub const DOOR_UNLOCKED: u32 = 0x1c2a5c;
    /// Every colour token: `(name, group, seat, value)`.
    pub const ALL: &[(&str, &str, &str, u32)] = &[
        ("pitch", "ground", "none", PITCH),
        ("ink2", "ground", "none", INK2),
        ("slate", "ground", "none", SLATE),
        ("slab", "ground", "none", SLAB),
        ("dust", "ground", "none", DUST),
        ("bone", "ground", "none", BONE),
        ("cobalt", "accent", "none", COBALT),
        ("phosphor", "accent", "hacker", PHOSPHOR),
        ("signal", "accent", "infiltrator", SIGNAL),
        ("neon", "accent", "none", NEON),
        ("alarm", "accent", "threat", ALARM),
        ("room_fill", "scene", "none", ROOM_FILL),
        ("room_fill_lit", "scene", "none", ROOM_FILL_LIT),
        ("room_stroke", "scene", "none", ROOM_STROKE),
        ("sky_top", "scene", "none", SKY_TOP),
        ("sky_low", "scene", "none", SKY_LOW),
        ("skyline", "scene", "none", SKYLINE),
        ("rain", "scene", "none", RAIN),
        ("grade", "scene", "none", GRADE),
        ("door_locked", "scene", "none", DOOR_LOCKED),
        ("door_unlocked", "scene", "none", DOOR_UNLOCKED),
    ];
}

/// The bundled typefaces: family names and grids.
#[rustfmt::skip]
pub mod font {
    /// The default UI face: a terminal face on a 16 px grid. Every HUD line, log and screen label.
    pub const VT323: &str = "VT323";
    pub const VT323_FILE: &str = "VT323-Regular.ttf";
    pub const VT323_GRID_PX: u32 = 16;
    /// Blocky capitals on an 8 px grid; selectable with IDAPTIK_UI_FONT=silkscreen.
    pub const SILKSCREEN: &str = "Silkscreen";
    pub const SILKSCREEN_FILE: &str = "Silkscreen-Regular.ttf";
    pub const SILKSCREEN_GRID_PX: u32 = 8;
    /// A readable pixel sans on an 8 px grid; selectable with IDAPTIK_UI_FONT=pixelify.
    pub const PIXELIFY: &str = "Pixelify Sans";
    pub const PIXELIFY_FILE: &str = "PixelifySans.ttf";
    pub const PIXELIFY_GRID_PX: u32 = 8;
    /// The default face's family name.
    pub const DEFAULT: &str = VT323;
    /// Every typeface: `(name, family, file, grid_px, default)`.
    pub const ALL: &[(&str, &str, &str, u32, bool)] = &[
        ("vt323", VT323, VT323_FILE, VT323_GRID_PX, true),
        ("silkscreen", SILKSCREEN, SILKSCREEN_FILE, SILKSCREEN_GRID_PX, false),
        ("pixelify", PIXELIFY, PIXELIFY_FILE, PIXELIFY_GRID_PX, false),
    ];
}

/// The type ramp as requested, in px.
#[rustfmt::skip]
pub mod size {
    /// Wire port labels on the hacker's screens; console sign text.
    pub const PX10: u32 = 10;
    /// Key hints under the HUD; device labels in the net view.
    pub const PX11: u32 = 11;
    /// Body copy: the wiring help line, the net HUD, the role-select body.
    pub const PX12: u32 = 12;
    /// The netplay status line.
    pub const PX14: u32 = 14;
    /// Deck cards, CRT console text, role-select labels.
    pub const PX16: u32 = 16;
    /// The role-select subtitle.
    pub const PX18: u32 = 18;
    /// The end-of-run overlay.
    pub const PX22: u32 = 22;
    /// The role-select title.
    pub const PX24: u32 = 24;
    /// Every size token: `(name, value)`.
    pub const ALL: &[(&str, u32)] = &[
        ("px10", PX10),
        ("px11", PX11),
        ("px12", PX12),
        ("px14", PX14),
        ("px16", PX16),
        ("px18", PX18),
        ("px22", PX22),
        ("px24", PX24),
    ];
}

/// Spacing, in px.
#[rustfmt::skip]
pub mod space {
    /// Deck card border.
    pub const HAIRLINE: u32 = 1;
    /// Overlay panel border; the tight row gap inside a deck card.
    pub const RULE: u32 = 2;
    /// HUD row gap.
    pub const GAP_4: u32 = 4;
    /// HUD column gap; deck card row gap.
    pub const GAP_6: u32 = 6;
    /// HUD top inset; deck card padding.
    pub const INSET_8: u32 = 8;
    /// HUD side inset; deck panel padding; meter bar height.
    pub const INSET_10: u32 = 10;
    /// Overlay panel vertical padding.
    pub const PAD_16: u32 = 16;
    /// Deck panel column gap.
    pub const GAP_18: u32 = 18;
    /// Overlay panel horizontal padding.
    pub const PAD_28: u32 = 28;
    /// The log tail's offset from the bottom edge.
    pub const LOG_BOTTOM: u32 = 46;
    /// Meter label column width.
    pub const METER_LABEL: u32 = 70;
    /// Meter bar width.
    pub const METER_BAR: u32 = 180;
    /// Every space token: `(name, value)`.
    pub const ALL: &[(&str, u32)] = &[
        ("hairline", HAIRLINE),
        ("rule", RULE),
        ("gap_4", GAP_4),
        ("gap_6", GAP_6),
        ("inset_8", INSET_8),
        ("inset_10", INSET_10),
        ("pad_16", PAD_16),
        ("gap_18", GAP_18),
        ("pad_28", PAD_28),
        ("log_bottom", LOG_BOTTOM),
        ("meter_label", METER_LABEL),
        ("meter_bar", METER_BAR),
    ];
}

/// Corner radius, in px.
#[rustfmt::skip]
pub mod radius {
    /// Every corner is square; no rounded UI anywhere.
    pub const CORNER: u32 = 0;
    /// Every radius token: `(name, value)`.
    pub const ALL: &[(&str, u32)] = &[
        ("corner", CORNER),
    ];
}

/// The art grid.
#[rustfmt::skip]
pub mod grid {
    /// The floor camera runs at three screen pixels per art pixel.
    pub const SCREEN_PX_PER_ART_PX: u32 = 3;
    /// The infiltrator is drawn 48 art pixels tall.
    pub const FIGURE_ART_PX: u32 = 48;
    /// The infiltrator is 54 canvas units tall (48 / 54 art px per unit).
    pub const FIGURE_UNITS: u32 = 54;
    /// Every grid token: `(name, value)`.
    pub const ALL: &[(&str, u32)] = &[
        ("screen_px_per_art_px", SCREEN_PX_PER_ART_PX),
        ("figure_art_px", FIGURE_ART_PX),
        ("figure_units", FIGURE_UNITS),
    ];
}
