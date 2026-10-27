//! The brand's legibility rule over the pairs the frontend actually draws.
//!
//! Rule 3 of the brand handoff: text contrast is checked at 4.5:1 (3:1 for
//! large text and UI glyphs) in every palette, and failures are **flagged,
//! not silently fixed**. So this test pins the verdict for every pair, pass
//! or fail, as it stands on the committed palette: a pair listed as failing
//! is a known, documented failure (see docs/design/brand/README.md §5 and the
//! open decisions in inventory.md), and the day someone changes the palette or
//! the drawing so the verdict moves, this test fails too and the book gets
//! updated with it.
//!
//! Every row says where the pair is drawn, by file, or says "recorded" when it
//! is a combination the book rules on but the frontend does not draw today.
//! Pairs on the deck's glass (`screen::DECK_GLASS`, an off-palette literal)
//! are pinned in `idaptik-bevy`'s palette tests, which can read that constant.
//!
//! "Large" here is WCAG's: 24 px, or 18.66 px bold. No text in the game is
//! drawn that large except the role-select title, so every text row is held to
//! 4.5:1; the 3:1 column is the bar for glyphs, bars and shapes.
//!
//! Verdicts come from `idaptik_kernel::contrast` (exact integers); the f64
//! ratio is reported only to keep the assertion messages readable, and a
//! guard below refuses any pair close enough to a threshold that the two
//! could disagree.

use idaptik_brand::generated::colour::*;
use idaptik_brand::srgb::{self, BODY, LARGE};

/// One foreground on one background, and the verdicts the book records.
struct Pair {
    fg: (&'static str, u32),
    bg: (&'static str, u32),
    /// Where the frontend draws it, or "recorded".
    drawn: &'static str,
    /// Meets 4.5:1 (body text).
    body: bool,
    /// Meets 3:1 (large text, UI glyphs, bars, shapes).
    glyph: bool,
}

const fn pair(
    fg: (&'static str, u32),
    bg: (&'static str, u32),
    drawn: &'static str,
    body: bool,
    glyph: bool,
) -> Pair {
    Pair {
        fg,
        bg,
        drawn,
        body,
        glyph,
    }
}

const PITCH_: (&str, u32) = ("pitch", PITCH);
const SLATE_: (&str, u32) = ("slate", SLATE);
const SLAB_: (&str, u32) = ("slab", SLAB);

const PAIRS: &[Pair] = &[
    // --- Text on the window clear colour (hud.rs).
    pair(
        ("bone", BONE),
        PITCH_,
        "hud.rs: status, meter labels, log",
        true,
        true,
    ),
    pair(("dust", DUST), PITCH_, "hud.rs: key hints", true, true),
    pair(
        ("neon", NEON),
        PITCH_,
        "hud.rs: end-of-run overlay text",
        true,
        true,
    ),
    pair(
        ("phosphor", PHOSPHOR),
        PITCH_,
        "recorded: the hacker's seat colour as text",
        true,
        true,
    ),
    pair(
        ("signal", SIGNAL),
        PITCH_,
        "recorded: the infiltrator's seat colour as text",
        true,
        true,
    ),
    // FLAGGED, live: screen.rs draws LOCKED / NO POWER device labels and the
    // GUARD estimate in alarm red at a requested 12-14 px (16 px drawn). It
    // fails 4.5:1. Open decision in inventory.md; not changed here (rule 3).
    pair(
        ("alarm", ALARM),
        PITCH_,
        "screen.rs: LOCKED / NO POWER labels, GUARD estimate (FLAGGED)",
        false,
        true,
    ),
    pair(
        ("cobalt", COBALT),
        PITCH_,
        "recorded: cobalt as text",
        false,
        true,
    ),
    // --- The five HUD meter fills on their slate track (hud.rs). Bars: the
    // 3:1 glyph bar applies.
    pair(
        ("phosphor", PHOSPHOR),
        SLATE_,
        "hud.rs: support meter fill",
        true,
        true,
    ),
    // FLAGGED, live: the bandwidth fill is cobalt on the slate track, 2.58:1,
    // under even the 3:1 glyph bar. Open decision in inventory.md.
    pair(
        ("cobalt", COBALT),
        SLATE_,
        "hud.rs: bandwidth meter fill (FLAGGED)",
        false,
        false,
    ),
    pair(
        ("alarm", ALARM),
        SLATE_,
        "hud.rs: alert meter fill",
        false,
        true,
    ),
    pair(
        ("dust", DUST),
        SLATE_,
        "hud.rs: isolation meter fill",
        true,
        true,
    ),
    pair(
        ("neon", NEON),
        SLATE_,
        "hud.rs: trace meter fill",
        true,
        true,
    ),
    // --- Type on concrete and rooms: recorded, so a new text site can be
    // checked against the book before it is drawn.
    pair(("bone", BONE), SLATE_, "recorded", true, true),
    pair(("bone", BONE), SLAB_, "recorded", true, true),
    pair(
        ("dust", DUST),
        SLAB_,
        "recorded: never set dust type on slab",
        false,
        true,
    ),
    pair(
        ("bone", BONE),
        ("room_fill", ROOM_FILL),
        "recorded",
        true,
        true,
    ),
    pair(
        ("bone", BONE),
        ("room_fill_lit", ROOM_FILL_LIT),
        "recorded",
        true,
        true,
    ),
    pair(
        ("phosphor", PHOSPHOR),
        ("room_fill", ROOM_FILL),
        "recorded",
        true,
        true,
    ),
    // --- Structure against night: surfaces, never text. Recorded so nobody
    // promotes them to text without seeing the number.
    pair(
        ("slab", SLAB),
        PITCH_,
        "scene.rs: door frames (surface)",
        false,
        false,
    ),
    pair(
        ("rain", RAIN),
        PITCH_,
        "art.rs: rain streaks (surface)",
        false,
        true,
    ),
    pair(
        ("room_stroke", ROOM_STROKE),
        PITCH_,
        "recorded: room outline (surface)",
        false,
        false,
    ),
];

#[test]
fn every_pair_has_the_verdict_the_book_records() {
    let mut failures = Vec::new();
    for p in PAIRS {
        let got_body = srgb::meets(p.fg.1, p.bg.1, BODY);
        let got_glyph = srgb::meets(p.fg.1, p.bg.1, LARGE);
        let r = srgb::ratio(p.fg.1, p.bg.1);
        if got_body != p.body || got_glyph != p.glyph {
            failures.push(format!(
                "{} on {} ({}): {r:.2}:1 -> body {got_body} / glyph {got_glyph}, book says body {} / glyph {}",
                p.fg.0, p.bg.0, p.drawn, p.body, p.glyph
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "verdicts moved; update the brand book:\n{}",
        failures.join("\n")
    );
}

#[test]
fn no_pair_sits_on_a_threshold() {
    // The integer verdict rounds channels to 1e-6; a ratio within 1e-3 of a
    // threshold would make the verdict a rounding artefact rather than a fact.
    for p in PAIRS {
        let r = srgb::ratio(p.fg.1, p.bg.1);
        for t in [4.5, 3.0] {
            assert!(
                (r - t).abs() > 1e-3,
                "{} on {} is {r} -- too close to {t}",
                p.fg.0,
                p.bg.0
            );
        }
    }
}

#[test]
fn the_table_names_its_tokens_truthfully() {
    // A row's label must be the token it tests, or a failure message would
    // blame the wrong colour.
    for p in PAIRS {
        for (name, value) in [p.fg, p.bg] {
            let token = ALL.iter().find(|t| t.0 == name);
            assert_eq!(token.map(|t| t.3), Some(value), "{name} is mislabelled");
        }
    }
}

#[test]
fn the_seat_law_colours_are_distinguishable_from_each_other() {
    // The seat law puts phosphor (hacker), signal (infiltrator) and alarm
    // (threat) side by side on screens. They need not pass as text on one
    // another, but each must clear the UI-glyph bar against the ground and
    // all three must differ in luminance by more than a rounding step.
    for (name, c) in [("phosphor", PHOSPHOR), ("signal", SIGNAL), ("alarm", ALARM)] {
        assert!(
            srgb::meets(c, PITCH, LARGE),
            "{name} must read as a glyph on night"
        );
    }
    let ys = [
        srgb::luminance(PHOSPHOR),
        srgb::luminance(SIGNAL),
        srgb::luminance(ALARM),
    ];
    for i in 0..3 {
        for j in (i + 1)..3 {
            assert!(
                ys[i].abs_diff(ys[j]) > 1_000_000,
                "seat colours {i} and {j} share a luminance"
            );
        }
    }
}

#[test]
fn every_colour_token_is_in_all_and_distinct() {
    let mut seen = std::collections::BTreeSet::new();
    for &(name, _, _, value) in ALL {
        assert!(
            seen.insert(value),
            "{name} repeats the value {value:06x} of another token"
        );
    }
    assert_eq!(
        ALL.len(),
        21,
        "the palette has 21 colours on 2026-10-27; update this with the book"
    );
}
