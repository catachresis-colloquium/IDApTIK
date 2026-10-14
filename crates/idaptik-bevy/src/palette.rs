//! The master palette. Every colour the frontend draws comes from here, so the
//! seat law can be audited in one file: **phosphor (acid mint) is the hacker's
//! and lives on screens; signal yellow is the infiltrator's; alarm red is
//! threat and the Machine.**
//!
//! The look is early-1980s new wave: a cold, minimal ground of off-black,
//! concrete grey and off-white, and a few hard flat accents — signal red,
//! signal yellow, cobalt, acid mint — never soft, never many at once.

use bevy::prelude::Color;

const fn hex(rgb: u32) -> Color {
    Color::srgb(
        ((rgb >> 16) & 0xff) as f32 / 255.0,
        ((rgb >> 8) & 0xff) as f32 / 255.0,
        (rgb & 0xff) as f32 / 255.0,
    )
}

/// Base night; interiors unlit. Off-black, not blue.
pub const PITCH: Color = hex(0x0c0c0e);
pub const INK2: Color = hex(0x141416);
/// Concrete, road, structure.
pub const SLATE: Color = hex(0x26262a);
pub const SLAB: Color = hex(0x36363b);
/// Cobalt: civic, open, safe passage.
pub const COBALT: Color = hex(0x2f5bd6);
/// The hacker's seat. Screens only.
pub const PHOSPHOR: Color = hex(0x5cf2b4);
/// Signal yellow: the infiltrator's seat, trim, and anything theirs.
pub const SIGNAL: Color = hex(0xffd21f);
/// Cold fluorescent light: room light, windows, street lamps.
pub const NEON: Color = hex(0xe9f1ff);
/// Signal red: threat, alarm, the Machine.
pub const ALARM: Color = hex(0xe8202a);
/// Paper, type, fiche.
pub const BONE: Color = hex(0xf2f0ea);
pub const DUST: Color = hex(0x8e8e94);

/// Room interior and its stroke.
pub const ROOM_FILL: Color = hex(0x18181b);
pub const ROOM_FILL_LIT: Color = hex(0x202024);
pub const ROOM_STROKE: Color = hex(0x4a4a50);
/// Sky gradient ends and the skyline silhouette.
pub const SKY_TOP: Color = hex(0x0f0f13);
pub const SKY_LOW: Color = hex(0x1e1e25);
pub const SKYLINE: Color = hex(0x0a0a0c);
/// Rain streaks.
pub const RAIN: Color = hex(0x6b6b78);
/// Ground grade line.
pub const GRADE: Color = hex(0x48484f);
/// Door slabs: locked and unlocked fills.
pub const DOOR_LOCKED: Color = hex(0x5a1418);
pub const DOOR_UNLOCKED: Color = hex(0x1c2a5c);
