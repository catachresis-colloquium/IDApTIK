//! The master palette. Every colour the frontend draws comes from here, so the
//! seat law can be audited in one file: **phosphor (acid mint) is the hacker's
//! and lives on screens; signal yellow is the infiltrator's; alarm red is
//! threat and the Machine.**
//!
//! The look is early-1980s new wave: a cold, minimal ground of off-black,
//! concrete grey and off-white, and a few hard flat accents — signal red,
//! signal yellow, cobalt, acid mint — never soft, never many at once.
//!
//! The values are not written here any more: they come from the brand tokens
//! (`config/brand-tokens.ncl`, exported to `crates/idaptik-brand/tokens.json`
//! and generated into `idaptik_brand::generated::colour`), and this file is
//! the `Color` view over them. The names, grouping and usage rules are the
//! brand book's (`docs/design/brand/README.md`); the legibility of every pair
//! the HUD draws is asserted in `crates/idaptik-brand/tests/legibility.rs`.

use bevy::prelude::Color;
use idaptik_brand::generated::colour as tok;

/// A brand token (`0xRRGGBB`) as a Bevy sRGB colour.
pub const fn hex(rgb: u32) -> Color {
    Color::srgb(
        ((rgb >> 16) & 0xff) as f32 / 255.0,
        ((rgb >> 8) & 0xff) as f32 / 255.0,
        (rgb & 0xff) as f32 / 255.0,
    )
}

/// Base night; interiors unlit. Off-black, not blue.
pub const PITCH: Color = hex(tok::PITCH);
pub const INK2: Color = hex(tok::INK2);
/// Concrete, road, structure.
pub const SLATE: Color = hex(tok::SLATE);
pub const SLAB: Color = hex(tok::SLAB);
/// Cobalt: civic, open, safe passage.
pub const COBALT: Color = hex(tok::COBALT);
/// The hacker's seat. Screens only.
pub const PHOSPHOR: Color = hex(tok::PHOSPHOR);
/// Signal yellow: the infiltrator's seat, trim, and anything theirs.
pub const SIGNAL: Color = hex(tok::SIGNAL);
/// Cold fluorescent light: room light, windows, street lamps.
pub const NEON: Color = hex(tok::NEON);
/// Signal red: threat, alarm, the Machine.
pub const ALARM: Color = hex(tok::ALARM);
/// Paper, type, fiche.
pub const BONE: Color = hex(tok::BONE);
pub const DUST: Color = hex(tok::DUST);

/// Room interior and its stroke.
pub const ROOM_FILL: Color = hex(tok::ROOM_FILL);
pub const ROOM_FILL_LIT: Color = hex(tok::ROOM_FILL_LIT);
pub const ROOM_STROKE: Color = hex(tok::ROOM_STROKE);
/// Sky gradient ends and the skyline silhouette.
pub const SKY_TOP: Color = hex(tok::SKY_TOP);
pub const SKY_LOW: Color = hex(tok::SKY_LOW);
pub const SKYLINE: Color = hex(tok::SKYLINE);
/// Rain streaks.
pub const RAIN: Color = hex(tok::RAIN);
/// Ground grade line.
pub const GRADE: Color = hex(tok::GRADE);
/// Door slabs: locked and unlocked fills.
pub const DOOR_LOCKED: Color = hex(tok::DOOR_LOCKED);
pub const DOOR_UNLOCKED: Color = hex(tok::DOOR_UNLOCKED);

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::color::Alpha;
    use idaptik_brand::srgb::{self, BODY, LARGE};

    /// A colour as the 8-bit sRGB bytes it renders to, `0xRRGGBB`.
    fn bytes(c: Color) -> u32 {
        let s = c.to_srgba();
        (u32::from((s.red * 255.0).round() as u8) << 16)
            | (u32::from((s.green * 255.0).round() as u8) << 8)
            | u32::from((s.blue * 255.0).round() as u8)
    }

    #[test]
    fn hex_round_trips_through_bevy_srgb_bytes() {
        // Every token must survive the f32 trip: converting back to 8-bit sRGB
        // gives the exact bytes the token names, for every token, not just
        // the ones this file names.
        for &(name, _, _, rgb) in tok::ALL {
            assert_eq!(bytes(hex(rgb)), rgb, "{name}");
        }
    }

    #[test]
    fn deck_glass_text_has_the_verdicts_the_book_records() {
        // The hacker's screens draw on `screen::DECK_GLASS`, an off-palette
        // literal (inventory.md), so these pairs are pinned here, where that
        // constant is readable, rather than in idaptik-brand's legibility test.
        let glass = bytes(crate::screen::DECK_GLASS);
        assert_eq!(
            glass, 0x07110d,
            "DECK_GLASS moved; re-check every row below"
        );

        // Translucent colours are composited over the glass with the alpha
        // the frontend really uses (`screen::ROOM_NAME_ALPHA`, `deck::FAINT`),
        // so retuning either moves the verdict here. The blend space decides
        // the result (room names: 4.72:1 in sRGB bytes, 7.89:1 in linear);
        // pin the worse one, the sRGB-space blend, so the verdict holds
        // either way.
        let over = |fg: Color, bg: u32| -> u32 {
            let a = f64::from(fg.alpha());
            let f = bytes(fg.with_alpha(1.0));
            let ch = |shift: u32| {
                let (x, y) = (
                    f64::from((f >> shift) & 0xff),
                    f64::from((bg >> shift) & 0xff),
                );
                ((a * x + (1.0 - a) * y).round() as u32) << shift
            };
            ch(16) | ch(8) | ch(0)
        };
        let dim = over(PHOSPHOR.with_alpha(crate::screen::ROOM_NAME_ALPHA), glass);
        let faint = over(crate::deck::FAINT, glass);

        let rows: [(&str, u32, u32, bool, bool); 9] = [
            (
                "phosphor device labels, screen.rs",
                bytes(PHOSPHOR),
                glass,
                true,
                true,
            ),
            (
                "signal INF estimate, screen.rs",
                bytes(SIGNAL),
                glass,
                true,
                true,
            ),
            (
                "dimmed phosphor room names, screen.rs",
                dim,
                glass,
                true,
                true,
            ),
            // FLAGGED, live: LOCKED / NO POWER labels and the GUARD estimate
            // (4.27:1). Open decision in inventory.md; not changed (rule 3).
            (
                "alarm LOCKED / NO POWER / GUARD text, screen.rs",
                bytes(ALARM),
                glass,
                false,
                true,
            ),
            (
                "glass-coloured verb on the phosphor card, deck.rs",
                glass,
                bytes(PHOSPHOR),
                true,
                true,
            ),
            (
                "dust as text on the glass (recorded)",
                bytes(DUST),
                glass,
                true,
                true,
            ),
            (
                "bone as text on the glass (recorded)",
                bytes(BONE),
                glass,
                true,
                true,
            ),
            // FLAGGED, live: an OT verb turns the ready button alarm red with
            // the verb still in glass (4.27:1). No palette colour reaches
            // 4.5:1 on alarm red. Open decision in inventory.md.
            (
                "glass-coloured OT verb on the alarm button, deck.rs",
                glass,
                bytes(ALARM),
                false,
                true,
            ),
            // FLAGGED, live: a not-ready button is FAINT (phosphor at 14 %)
            // and its verb is about 1.35:1 (2.77:1 in a linear blend). It is
            // NOT a disabled control: `card_press` still queues the NetHack,
            // and the sim counts a failed action and emits NetHackDenied, so
            // WCAG 1.4.3's inactive-control exemption does not apply. Open
            // decision in inventory.md; not changed (rule 3).
            (
                "glass-coloured verb on the not-ready button (live, pressable), deck.rs",
                glass,
                faint,
                false,
                false,
            ),
        ];
        for (what, fg, bg, body, glyph) in rows {
            let r = srgb::ratio(fg, bg);
            assert_eq!(srgb::meets(fg, bg, BODY), body, "{what}: {r:.2}:1 at 4.5:1");
            assert_eq!(srgb::meets(fg, bg, LARGE), glyph, "{what}: {r:.2}:1 at 3:1");
            for t in [4.5, 3.0] {
                assert!((r - t).abs() > 1e-3, "{what}: {r} sits on {t}");
            }
        }
    }

    #[test]
    fn this_file_names_every_colour_token() {
        // The `Color` view must not fall behind the token set: one const per
        // token, and the count pins it.
        const VIEW: &[Color] = &[
            PITCH,
            INK2,
            SLATE,
            SLAB,
            COBALT,
            PHOSPHOR,
            SIGNAL,
            NEON,
            ALARM,
            BONE,
            DUST,
            ROOM_FILL,
            ROOM_FILL_LIT,
            ROOM_STROKE,
            SKY_TOP,
            SKY_LOW,
            SKYLINE,
            RAIN,
            GRADE,
            DOOR_LOCKED,
            DOOR_UNLOCKED,
        ];
        assert_eq!(
            VIEW.len(),
            tok::ALL.len(),
            "a colour token has no Color const here"
        );
    }
}
