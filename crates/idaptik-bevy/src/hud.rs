//! The HUD: status line, meter bars (support / bandwidth / alert / isolation /
//! trace), the event-log tail, the key bindings, and the end-of-run overlay.
//! Every value shown is read straight off the sim; nothing is re-derived.

use bevy::prelude::*;
use idaptik_core::scenario::GhostLobbySim;
use idaptik_core::scenario::common::Channel;

use crate::driver::SimState;

/// Which meter a fill node shows.
#[derive(Clone, Copy)]
pub enum Meter {
    Support,
    Bandwidth,
    Alert,
    Isolation,
    Trace,
}

impl Meter {
    const ALL: [Meter; 5] = [
        Meter::Support,
        Meter::Bandwidth,
        Meter::Alert,
        Meter::Isolation,
        Meter::Trace,
    ];

    fn label(self) -> &'static str {
        match self {
            Meter::Support => "support",
            Meter::Bandwidth => "bandwidth",
            Meter::Alert => "alert",
            Meter::Isolation => "isolation",
            Meter::Trace => "trace",
        }
    }

    fn color(self) -> Color {
        match self {
            Meter::Support => crate::palette::PHOSPHOR,
            Meter::Bandwidth => crate::palette::COBALT,
            Meter::Alert => crate::palette::ALARM,
            Meter::Isolation => crate::palette::DUST,
            Meter::Trace => crate::palette::NEON,
        }
    }

    /// The meter's 0..=1 fill, read from sim state exactly as the TUI reads it.
    fn value(self, sim: &GhostLobbySim) -> f64 {
        let s = sim.state();
        match self {
            Meter::Support => s.support,
            Meter::Bandwidth => s.bandwidth / 100.0,
            Meter::Alert => s.alert / 100.0,
            // Isolation is meaningful as a fraction of the per-difficulty
            // support limit, which is where the run fails.
            Meter::Isolation => s.isolation / support_limit(sim),
            Meter::Trace => f64::from(s.agents.hacker.trace_fraction()),
        }
    }
}

/// The per-difficulty support limit (isolation's failure threshold).
fn support_limit(sim: &GhostLobbySim) -> f64 {
    sim.definition()
        .difficulty
        .get(&sim.config().difficulty)
        .map(|p| p.support_limit)
        .unwrap_or(1.0)
        .max(0.001)
}

/// Marker: a meter's fill node.
#[derive(Component)]
pub struct MeterFill(pub Meter);
/// Marker: the status line.
#[derive(Component)]
pub struct StatusText;
/// Marker: the event-log tail.
#[derive(Component)]
pub struct LogText;
/// Marker: the end-of-run overlay.
#[derive(Component)]
pub struct ResultText;
/// Marker: the panel behind the end-of-run overlay, shown only once it ends.
#[derive(Component)]
pub struct ResultPanel;
/// Marker: the Ghost Lobby HUD's top-level entities, shown/hidden as a block
/// alongside `scene::GhostLobbySceneMarker`.
#[derive(Component)]
pub struct GhostLobbyHudMarker;

fn hud_text(size: f32) -> (TextFont, TextColor) {
    (
        TextFont::from_font_size(size),
        TextColor(crate::palette::BONE),
    )
}

/// Spawn the HUD tree: status + meters top-left, log tail and key hints at the
/// bottom, the (initially empty) result overlay in the centre.
pub fn setup_hud(mut commands: Commands) {
    commands
        .spawn((
            GhostLobbyHudMarker,
            crate::deck::InPane::hud(Some(10.0), Some(8.0), None, None),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(8.0),
                left: Val::Px(10.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..Default::default()
            },
        ))
        .with_children(|root| {
            root.spawn((Text::new(""), hud_text(13.0), StatusText));
            for meter in Meter::ALL {
                root.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(6.0),
                    align_items: AlignItems::Center,
                    ..Default::default()
                })
                .with_children(|row| {
                    row.spawn((
                        Text::new(meter.label()),
                        hud_text(11.0),
                        Node {
                            width: Val::Px(70.0),
                            ..Default::default()
                        },
                    ));
                    row.spawn((
                        Node {
                            width: Val::Px(180.0),
                            height: Val::Px(10.0),
                            ..Default::default()
                        },
                        BackgroundColor(crate::palette::SLATE),
                    ))
                    .with_children(|bar| {
                        bar.spawn((
                            Node {
                                width: Val::Percent(0.0),
                                height: Val::Percent(100.0),
                                ..Default::default()
                            },
                            BackgroundColor(meter.color()),
                            MeterFill(meter),
                        ));
                    });
                });
            }
        });

    commands.spawn((
        GhostLobbyHudMarker,
        crate::deck::InPane::hud(Some(10.0), None, Some(10.0), Some(46.0)),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(46.0),
            left: Val::Px(10.0),
            ..Default::default()
        },
        Text::new(""),
        hud_text(12.0),
        LogText,
    ));

    // The guard-brain developer view, drawn top-right. It shows what the guard
    // perceived, what the kernel proposed, and what the host did with it —
    // including whether the proposal was refused and why.
    #[cfg(feature = "weevil")]
    commands.spawn((
        GhostLobbyHudMarker,
        crate::deck::InPane::hud(Some(10.0), Some(160.0), Some(10.0), None),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(8.0),
            right: Val::Px(10.0),
            ..Default::default()
        },
        Text::new(""),
        hud_text(11.0),
        GuardBrainText,
    ));

    commands.spawn((
        GhostLobbyHudMarker,
        crate::deck::InPane::hud(Some(10.0), None, Some(10.0), Some(8.0)),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(8.0),
            left: Val::Px(10.0),
            ..Default::default()
        },
        Text::new(
            "A/D move  Shift sprint  W/Space jump  S crouch/hide  E interact  Q throw  \
             mouse: the hacker  H deck  B both/floor/van  Tab pause  R restart  Esc quit",
        ),
        TextFont::from_font_size(11.0),
        TextColor(crate::palette::DUST),
    ));

    // The result sits on its own dark panel, above every other UI node, so
    // it reads over any scene.
    commands
        .spawn((
            GhostLobbyHudMarker,
            GlobalZIndex(100),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(36.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..Default::default()
            },
        ))
        .with_children(|row| {
            row.spawn((
                ResultPanel,
                Node {
                    display: Display::None,
                    padding: UiRect::axes(Val::Px(28.0), Val::Px(16.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    max_width: Val::Percent(80.0),
                    ..Default::default()
                },
                BackgroundColor(crate::palette::PITCH.with_alpha(0.96)),
                BorderColor::all(crate::palette::SLAB),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new(""),
                    TextFont::from_font_size(22.0),
                    TextColor(crate::palette::NEON),
                    TextLayout::justify(Justify::Center),
                    ResultText,
                ));
            });
        });
}

/// Drive the meter fills from sim state.
pub fn update_meters(sim: Res<SimState>, mut fills: Query<(&MeterFill, &mut Node)>) {
    for (fill, mut node) in &mut fills {
        let value = fill.0.value(&sim.sim).clamp(0.0, 1.0);
        node.width = Val::Percent((value * 100.0) as f32);
    }
}

/// The status line: phase, clock, Billy's mode, seed, and the hacker's uplink
/// vantage / pivot depth / reach — the readout the pivot keys exist to move.
pub fn update_status_text(sim: Res<SimState>, mut text: Query<&mut Text, With<StatusText>>) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let s = sim.sim.state();
    let hacker = &s.agents.hacker;
    let secs = s.t as u64;
    let paused = if sim.sim.is_paused() {
        "   [PAUSED]"
    } else {
        ""
    };
    text.0 = format!(
        "Envelope 001 — Ghost Lobby   phase {:?}   {:02}:{:02}   Billy: {:?}   seed {:08X}{}\n\
         uplink: vantage {:?}   pivot depth {}   nodes reachable {}",
        s.phase,
        secs / 60,
        secs % 60,
        s.billy.mode,
        sim.sim.seed(),
        paused,
        hacker.vantage().kind,
        hacker.hops(),
        hacker.reachable_count(sim.sim.graph()),
    );
}

/// The last few human log lines (the same filtered view the TUI shows).
pub fn update_log_text(sim: Res<SimState>, mut text: Query<&mut Text, With<LogText>>) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let tail: Vec<String> = sim
        .log
        .iter()
        .filter(|l| matches!(l.channel, Channel::Log | Channel::Telemetry))
        .rev()
        .take(4)
        .map(|l| {
            let secs = l.t as u64;
            format!("{:02}:{:02}  {}", secs / 60, secs % 60, l.text)
        })
        .collect();
    text.0 = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
}

/// The end-of-run overlay: grade, score and debrief summary once the sim ends.
pub fn update_result_text(
    sim: Res<SimState>,
    mut text: Query<&mut Text, With<ResultText>>,
    mut panel: Query<&mut Node, With<ResultPanel>>,
) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    if let Ok(mut node) = panel.single_mut() {
        let display = if sim.sim.is_ended() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    if !sim.sim.is_ended() {
        if !text.0.is_empty() {
            text.0.clear();
        }
        return;
    }
    let Some(d) = sim.sim.debrief() else {
        return;
    };
    let verdict = if d.success { "EXTRACTED" } else { "FAILED" };
    text.0 = format!(
        "{verdict} — GRADE {:?}   SCORE {}\n{}\n{}\n(R restarts from the same seed)",
        d.grade, d.score, d.title, d.summary
    );
}

/// The guard-brain developer view: what the guard perceived, what the kernel
/// proposed, and what the host did with it. Every line is read straight off
/// the adapter; nothing is re-derived.
#[cfg(feature = "weevil")]
#[derive(Component)]
pub struct GuardBrainText;

/// Refresh the guard-brain panel from the adapter's own state.
///
/// When the weevil feature is off the panel is not spawned and this system is
/// not registered, so a classic run shows nothing new.
#[cfg(feature = "weevil")]
pub fn update_guard_brain_text(
    guard: Option<Res<crate::guard_weevil::GuardBrain>>,
    mut text: Query<&mut Text, With<GuardBrainText>>,
) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let Some(guard) = guard else {
        text.0 = "guard: classic fsm".to_owned();
        return;
    };
    let s = &guard.state;
    let profile = s.profile_id.clone().unwrap_or_else(|| "?".to_owned());
    let version = s
        .kernel_version
        .map(|v| format!("{}.{}.{}", v.0, v.1, v.2))
        .unwrap_or_else(|| "-".to_owned());
    let contacts = if s.contacts.is_empty() {
        "none".to_owned()
    } else {
        s.contacts
            .iter()
            .map(|c| format!("{}#{}({})", c.source, c.id, c.distance as i64))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let decision = s
        .last_decision
        .as_ref()
        .map(|d| format!("{} t={}", d.action, d.target_id))
        .unwrap_or_else(|| "-".to_owned());
    let outcome = s
        .last_outcome
        .map(|o| {
            if o.accepted {
                format!("applied/{}", o.reason)
            } else {
                format!("refused/{}", o.reason)
            }
        })
        .unwrap_or_else(|| "-".to_owned());
    let error = s.last_error.as_deref().map(|e| format!(" ERR {e}")).unwrap_or_default();
    text.0 = format!("weevil {profile} v{version}\nsee: {contacts}\nwant: {decision}\nhost: {outcome}{error}");
}
