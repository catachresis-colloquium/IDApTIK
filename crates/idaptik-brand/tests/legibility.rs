//! The brand's legibility rule over the pairs the frontend actually draws.
//!
//! Rule 3 of the brand handoff: text contrast is checked at 4.5:1 (3:1 for
//! large text and UI glyphs) in every palette, and failures are **flagged,
//! not silently fixed**. So this test pins the verdict for every pair, pass
//! or fail, as it stands on the committed palette: a pair listed as failing
//! is a known, documented failure (see docs/design/brand/README.md, "What
//! fails"), and the day someone changes the palette so it passes, this test
//! fails too and the book gets updated with it.
//!
//! Verdicts come from `idaptik_kernel::contrast` (exact integers); the f64
//! ratio is reported only to keep the assertion messages readable, and a
//! guard below refuses any pair close enough to a threshold that the two
//! could disagree.

use idaptik_brand::generated::colour::*;
use idaptik_brand::srgb::{self, BODY, LARGE};

/// `(foreground, background, where it is drawn, meets 4.5:1, meets 3:1)`.
const PAIRS: &[(&str, u32, &str, u32, &str, bool, bool)] = &[
    // Text the HUD draws on the window clear colour.
    (
        "bone",
        BONE,
        "pitch",
        PITCH,
        "HUD status, meters and log (hud_text)",
        true,
        true,
    ),
    (
        "dust",
        DUST,
        "pitch",
        PITCH,
        "key hints under the HUD",
        true,
        true,
    ),
    (
        "neon",
        NEON,
        "pitch",
        PITCH,
        "end-of-run overlay text on the PITCH panel",
        true,
        true,
    ),
    (
        "phosphor",
        PHOSPHOR,
        "pitch",
        PITCH,
        "wiring labels and help line on the hacker's screens",
        true,
        true,
    ),
    (
        "signal",
        SIGNAL,
        "pitch",
        PITCH,
        "the infiltrator's trim and labels",
        true,
        true,
    ),
    // Accents as text on the ground: alarm red is legible as a glyph, not as body copy.
    (
        "alarm",
        ALARM,
        "pitch",
        PITCH,
        "alarm as text (not drawn as text today; flagged)",
        false,
        true,
    ),
    (
        "cobalt",
        COBALT,
        "pitch",
        PITCH,
        "bandwidth meter bar (UI glyph)",
        false,
        true,
    ),
    // Text on the concrete greys.
    (
        "bone",
        BONE,
        "slate",
        SLATE,
        "meter label on the SLATE meter track",
        true,
        true,
    ),
    (
        "bone",
        BONE,
        "slab",
        SLAB,
        "type on a door frame or prop",
        true,
        true,
    ),
    (
        "dust",
        DUST,
        "slate",
        SLATE,
        "isolation meter bar on its SLATE track (UI glyph)",
        true,
        true,
    ),
    (
        "dust",
        DUST,
        "slab",
        SLAB,
        "dust as text on SLAB (flagged: below 4.5:1)",
        false,
        true,
    ),
    // Text inside rooms.
    (
        "bone",
        BONE,
        "room_fill",
        ROOM_FILL,
        "type over an unlit room",
        true,
        true,
    ),
    (
        "bone",
        BONE,
        "room_fill_lit",
        ROOM_FILL_LIT,
        "type over a lit room",
        true,
        true,
    ),
    (
        "phosphor",
        PHOSPHOR,
        "room_fill",
        ROOM_FILL,
        "a phosphor screen inside a room",
        true,
        true,
    ),
    // Structure against ground: these are surfaces, never text, recorded so
    // nobody promotes them to text without seeing the number.
    (
        "slab",
        SLAB,
        "pitch",
        PITCH,
        "door frame on night (surface, not text)",
        false,
        false,
    ),
    (
        "rain",
        RAIN,
        "pitch",
        PITCH,
        "rain streaks on night (surface, not text)",
        false,
        true,
    ),
    (
        "room_stroke",
        ROOM_STROKE,
        "pitch",
        PITCH,
        "room outline on night (surface, not text)",
        false,
        false,
    ),
];

#[test]
fn every_drawn_pair_has_the_verdict_the_book_records() {
    let mut failures = Vec::new();
    for &(fg_name, fg, bg_name, bg, context, body, large) in PAIRS {
        let got_body = srgb::meets(fg, bg, BODY);
        let got_large = srgb::meets(fg, bg, LARGE);
        let r = srgb::ratio(fg, bg);
        if got_body != body || got_large != large {
            failures.push(format!(
                "{fg_name} on {bg_name} ({context}): {r:.2}:1 -> body {got_body} / large {got_large}, book says body {body} / large {large}"
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
    for &(fg_name, fg, bg_name, bg, _, _, _) in PAIRS {
        let r = srgb::ratio(fg, bg);
        for t in [4.5, 3.0] {
            assert!(
                (r - t).abs() > 1e-3,
                "{fg_name} on {bg_name} is {r} -- too close to {t}"
            );
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
