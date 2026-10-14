//! The deck's controls and the split's chrome.
//!
//! Under the deck's schematic sits the card for the device selected on it:
//! what the system reports, and the verb the hacker can send — its risk, its
//! cost in bandwidth, and what it will do in the world. The button sends it
//! as the same `Command::NetHack` the network map uses; the sim decides
//! whether it lands.
//!
//! The chrome: a phosphor bezel round the deck layer, the divider between
//! the seats, and [`InPane`], which pins a UI node inside a seat's pane so
//! the HUD stays with the floor however the window is split.

use bevy::prelude::*;
use idaptik_core::scenario::console::{ScreenDevice, ScreenKind};
use idaptik_core::scenario::constants as c;
use idaptik_core::scenario::signal::sim::Seat;

use crate::driver::{CommandQueue, SimState};
use crate::palette::{PHOSPHOR, SLAB};
use crate::screen::{DECK_GLASS, DeckSelection, Screen, current_view, hack};
use crate::seats::{DIVIDER, Seats};

/// What a device's verb is, as the card shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verb {
    pub label: &'static str,
    /// 1 (passive) to 5 (heavy); what the action risks being noticed.
    pub risk: u8,
    /// Operational technology: touching it moves something in the world.
    pub ot: bool,
    pub note: &'static str,
}

/// The verb each kind of device answers to.
pub fn verb_for(kind: ScreenKind) -> Verb {
    match kind {
        ScreenKind::Door => Verb {
            label: "HOLD OPEN",
            risk: 2,
            ot: false,
            note: "The relay arms, the door opens a few seconds, then shuts. Logs an out-of-hours open.",
        },
        ScreenKind::Camera => Verb {
            label: "LOOP FEED",
            risk: 3,
            ot: false,
            note: "The feed repeats and the camera stops seeing. A loop is obvious on review.",
        },
        ScreenKind::Light => Verb {
            label: "CUT LIGHTS",
            risk: 2,
            ot: true,
            note: "The room goes black and its cameras go blind. A torch still works.",
        },
        ScreenKind::Vacuum => Verb {
            label: "RUN VACUUM",
            risk: 2,
            ot: true,
            note: "Noise on the floor. A guard within earshot goes to look.",
        },
    }
}

/// Risk as five pips, ASCII so every pixel font has them.
pub fn pips(risk: u8) -> String {
    (1..=5).map(|k| if k <= risk { '#' } else { '-' }).collect()
}

/// A UI node pinned inside a seat's pane: offsets from the pane's edges, in
/// logical pixels. `seat: None` means the floor's pane, or the van's when the
/// floor is not on screen (the HUD).
#[derive(Component, Debug, Clone, Copy)]
pub struct InPane {
    pub seat: Option<Seat>,
    pub left: Option<f32>,
    pub right: Option<f32>,
    pub top: Option<f32>,
    pub bottom: Option<f32>,
}

impl InPane {
    pub fn hud(
        left: Option<f32>,
        top: Option<f32>,
        right: Option<f32>,
        bottom: Option<f32>,
    ) -> Self {
        Self {
            seat: None,
            left,
            right,
            top,
            bottom,
        }
    }
}

/// Keep pinned nodes inside their panes.
pub fn place_in_panes(
    seats: Option<Res<Seats>>,
    windows: Query<&Window>,
    mut nodes: Query<(&InPane, &mut Node)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let full = Rect::new(0.0, 0.0, window.width(), window.height());
    for (pin, mut node) in &mut nodes {
        let pane = match (seats.as_deref(), pin.seat) {
            (None, _) => Some(full),
            (Some(s), Some(seat)) => s.panes.of(seat),
            (Some(s), None) => s.panes.infiltrator.or(s.panes.hacker),
        };
        let Some(r) = pane else {
            node.display = Display::None;
            continue;
        };
        node.display = Display::Flex;
        let px = |v: Option<f32>, edge: f32| v.map_or(Val::Auto, |v| Val::Px(edge + v));
        node.left = px(pin.left, r.min.x);
        node.top = px(pin.top, r.min.y);
        node.right = px(pin.right, full.max.x - r.max.x);
        node.bottom = px(pin.bottom, full.max.y - r.max.y);
    }
}

//## The card

#[derive(Component)]
pub struct DeckPanel;
#[derive(Component)]
pub struct DeckBezel;
#[derive(Component)]
pub struct Divider;
#[derive(Component)]
pub struct DeckHint;
#[derive(Component)]
pub struct CardVerb;
/// Which line of the card a text is.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardText {
    Title,
    State,
    Verb,
    Note,
}

const DIM: Color = Color::srgba(0.36, 0.95, 0.71, 0.55);
const FAINT: Color = Color::srgba(0.36, 0.95, 0.71, 0.14);

fn line(text: &str, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont::from_font_size(16.0),
        TextColor(color),
    )
}

/// Spawn the deck's panel and bezel, the divider and the van's hint.
pub fn setup_deck(mut commands: Commands) {
    let absolute = || Node {
        position_type: PositionType::Absolute,
        display: Display::None,
        ..Default::default()
    };
    commands.spawn((Divider, absolute(), BackgroundColor(SLAB)));
    commands.spawn((
        DeckBezel,
        Node {
            border: UiRect::all(Val::Px(2.0)),
            ..absolute()
        },
        BorderColor::all(DIM),
    ));
    commands.spawn((
        DeckHint,
        InPane {
            seat: Some(Seat::Hacker),
            left: Some(12.0),
            top: Some(8.0),
            right: None,
            bottom: None,
        },
        Node {
            position_type: PositionType::Absolute,
            ..Default::default()
        },
        line("VAN   H: open the deck", PHOSPHOR),
    ));
    commands
        .spawn((
            DeckPanel,
            Node {
                padding: UiRect::all(Val::Px(10.0)),
                column_gap: Val::Px(18.0),
                // The deck powers up from small: keep its text inside it.
                overflow: Overflow::clip(),
                ..absolute()
            },
            // No backdrop: the deck's camera draws the glass behind it.
        ))
        .with_children(|panel| {
            // The card: what is selected and what can be done to it.
            panel
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(6.0),
                    flex_grow: 1.0,
                    flex_basis: Val::Px(0.0),
                    padding: UiRect::all(Val::Px(8.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..Default::default()
                })
                .insert(BorderColor::all(DIM))
                .with_children(|card| {
                    card.spawn((CardText::Title, line("NO DEVICE", PHOSPHOR)));
                    card.spawn((CardText::State, line("", DIM)));
                    card.spawn((
                        CardVerb,
                        Button,
                        Node {
                            padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                            ..Default::default()
                        },
                        BackgroundColor(PHOSPHOR),
                    ))
                    .with_children(|b| {
                        b.spawn((CardText::Verb, line("", DECK_GLASS)));
                    });
                    card.spawn((CardText::Note, line("", DIM)));
                });
            // The keys the deck answers to.
            panel
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    flex_basis: Val::Px(0.0),
                    flex_grow: 0.7,
                    ..Default::default()
                })
                .with_children(|keys| {
                    for k in [
                        "click  select a device",
                        "H      close the deck",
                        "C      wiring",
                        "X      back out",
                        "B      both / floor / van",
                    ] {
                        keys.spawn(line(k, DIM));
                    }
                });
        });
}

fn place(node: &mut Node, r: Option<Rect>) {
    match r {
        Some(r) if r.width() >= 1.0 && r.height() >= 1.0 => {
            node.display = Display::Flex;
            node.left = Val::Px(r.min.x);
            node.top = Val::Px(r.min.y);
            node.width = Val::Px(r.width());
            node.height = Val::Px(r.height());
        }
        _ => node.display = Display::None,
    }
}

type VerbButton<'w, 's> = Query<
    'w,
    's,
    (&'static mut Node, &'static mut BackgroundColor),
    (
        With<CardVerb>,
        Without<Divider>,
        Without<DeckBezel>,
        Without<DeckPanel>,
    ),
>;

type ChromeNodes<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Node,
        Has<Divider>,
        Has<DeckBezel>,
        Has<DeckPanel>,
    ),
    Or<(With<Divider>, With<DeckBezel>, With<DeckPanel>)>,
>;

/// Lay the chrome over the panes, and fill the card.
#[allow(clippy::too_many_arguments)]
pub fn update_deck(
    sim: Res<SimState>,
    screen: Res<Screen>,
    seats: Option<Res<Seats>>,
    selection: Res<DeckSelection>,
    mut chrome: ChromeNodes,
    mut hint: Query<&mut Visibility, With<DeckHint>>,
    mut texts: Query<(&CardText, &mut Text)>,
    mut verb: VerbButton,
) {
    let panes = seats.as_ref().map(|s| s.panes).unwrap_or_default();
    let divider = match (panes.infiltrator, panes.hacker) {
        (Some(a), Some(b)) => Some(if (a.max.y - b.min.y).abs() < DIVIDER * 2.0 {
            Rect::new(a.min.x, a.max.y, a.max.x, b.min.y)
        } else {
            Rect::new(a.max.x, a.min.y, b.min.x, a.max.y)
        }),
        _ => None,
    };
    let bezel = match (panes.deck, panes.deck_ui) {
        (Some(d), Some(u)) => Some(d.union(u).inflate(3.0)),
        _ => None,
    };
    for (mut node, is_divider, is_bezel, is_panel) in &mut chrome {
        let r = if is_divider {
            divider
        } else if is_bezel {
            bezel
        } else if is_panel {
            panes.deck_ui
        } else {
            None
        };
        place(&mut node, r);
    }
    for mut vis in &mut hint {
        *vis = if screen.on(Seat::Hacker) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }

    let view = current_view(&sim, &screen, Seat::Hacker);
    let device: Option<&ScreenDevice> = view.as_ref().and_then(|v| {
        let node = selection.0.as_deref()?;
        v.devices.iter().find(|d| d.node == node)
    });
    let s = sim.sim.state();
    let Some(d) = device else {
        set(&mut texts, CardText::Title, "NO DEVICE".into());
        set(&mut texts, CardText::State, String::new());
        set(
            &mut texts,
            CardText::Note,
            "Click a door, a camera, a light or the vacuum on the schematic.".into(),
        );
        if let Ok((mut node, _)) = verb.single_mut() {
            node.display = Display::None;
        }
        return;
    };
    let v = verb_for(d.kind);
    set(
        &mut texts,
        CardText::Title,
        format!(
            "{}   {}",
            d.node.to_uppercase(),
            if v.ot { "OT" } else { "" }
        ),
    );
    set(&mut texts, CardText::State, d.status.clone());
    let ready = d.actionable && s.bandwidth >= c::NET_HACK_COST && s.net_hack_cd <= 0.0;
    let why = if !d.actionable {
        "Out of reach from here: pivot in (P).".to_owned()
    } else if s.net_hack_cd > 0.0 {
        format!("Session busy {:.1}s.", s.net_hack_cd)
    } else if s.bandwidth < c::NET_HACK_COST {
        "Not enough bandwidth.".to_owned()
    } else {
        String::new()
    };
    set(
        &mut texts,
        CardText::Verb,
        format!("{}   risk {}", v.label, pips(v.risk)),
    );
    set(
        &mut texts,
        CardText::Note,
        format!("bandwidth {:.0}   {}\n{}", c::NET_HACK_COST, why, v.note),
    );
    if let Ok((mut node, mut bg)) = verb.single_mut() {
        node.display = Display::Flex;
        let base = if v.ot {
            crate::palette::ALARM
        } else {
            PHOSPHOR
        };
        bg.0 = if ready { base } else { FAINT };
    }
}

/// Set one line of the card, touching it only when it changes.
fn set(texts: &mut Query<(&CardText, &mut Text)>, which: CardText, text: String) {
    for (field, mut t) in texts.iter_mut() {
        if *field == which && t.0 != text {
            t.0 = text.clone();
        }
    }
}

/// The card's button sends its verb.
pub fn card_press(
    sim: Res<SimState>,
    screen: Res<Screen>,
    selection: Res<DeckSelection>,
    buttons: Query<&Interaction, (Changed<Interaction>, With<CardVerb>)>,
    mut queue: ResMut<CommandQueue>,
) {
    if !buttons.iter().any(|i| *i == Interaction::Pressed) {
        return;
    }
    let Some(view) = current_view(&sim, &screen, Seat::Hacker) else {
        return;
    };
    if let Some(d) = selection
        .0
        .as_deref()
        .and_then(|n| view.devices.iter().find(|d| d.node == n))
        .filter(|d| d.actionable)
    {
        hack(&mut queue, d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_reads_as_five_pips() {
        assert_eq!(pips(2), "##---");
        assert_eq!(pips(5), "#####");
    }

    #[test]
    fn lights_and_the_vacuum_are_ot() {
        assert!(verb_for(ScreenKind::Light).ot);
        assert!(verb_for(ScreenKind::Vacuum).ot);
        assert!(!verb_for(ScreenKind::Door).ot);
        assert!(!verb_for(ScreenKind::Camera).ot);
    }
}
