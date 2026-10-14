//! Bevy keyboard state → wire [`Command`]s, mirroring the canonical bindings
//! in `idaptik-tui/src/keymap.rs`.
//!
//! Held movement (A/D/S/Shift/E) becomes `SetButton` press/release pairs;
//! discrete actions (W/Space, Q, E, 1–4, the pivots) fire on `just_pressed`.
//! Case is significant only on the pivot line: `p` is the building bridge,
//! `Shift+p` the ISP ops host, exactly as in the TUI.

use bevy::prelude::*;
use idaptik_core::scenario::ActionKind;
use idaptik_core::scenario::command::{Button, Command, PivotTarget};

use crate::driver::CommandQueue;
use crate::net_view::AppMode;

const LEFT: &[KeyCode] = &[KeyCode::KeyA, KeyCode::ArrowLeft];
const RIGHT: &[KeyCode] = &[KeyCode::KeyD, KeyCode::ArrowRight];
const CROUCH: &[KeyCode] = &[KeyCode::KeyS, KeyCode::ArrowDown];
const SPRINT: &[KeyCode] = &[KeyCode::ShiftLeft, KeyCode::ShiftRight];
const INTERACT: &[KeyCode] = &[KeyCode::KeyE];
const JUMP: &[KeyCode] = &[KeyCode::KeyW, KeyCode::ArrowUp, KeyCode::Space];

/// Decode this frame's keyboard state into queued commands. Returns `true` if
/// the user asked to quit (Escape).
pub fn decode(keys: &ButtonInput<KeyCode>, queue: &mut CommandQueue) -> bool {
    decode_infiltrator(keys, queue);
    decode_hacker(keys, queue);
    decode_session(keys, queue)
}

/// The infiltrator's body: movement, interact, jump, throw.
pub fn decode_infiltrator(keys: &ButtonInput<KeyCode>, queue: &mut CommandQueue) {
    // Held movement buttons.
    sync_hold(keys, queue, LEFT, Button::Left);
    sync_hold(keys, queue, RIGHT, Button::Right);
    sync_hold(keys, queue, CROUCH, Button::Crouch);
    sync_hold(keys, queue, SPRINT, Button::Sprint);
    sync_hold(keys, queue, INTERACT, Button::Interact);

    // Edges. Interact is BOTH a held button and an edge press, as in the TUI.
    if keys.just_pressed(KeyCode::KeyE) {
        queue.push(Command::Interact);
    }
    if JUMP.iter().any(|c| keys.just_pressed(*c)) {
        queue.push(Command::Jump);
    }
    if keys.just_pressed(KeyCode::KeyQ) {
        queue.push(Command::ThrowUsb);
    }
}

/// The hacker's verbs: the four uplinks and the pivots.
pub fn decode_hacker(keys: &ButtonInput<KeyCode>, queue: &mut CommandQueue) {
    // Uplink actions.
    for (code, kind) in [
        (KeyCode::Digit1, ActionKind::Camera),
        (KeyCode::Digit2, ActionKind::Door),
        (KeyCode::Digit3, ActionKind::Vacuum),
        (KeyCode::Digit4, ActionKind::Lights),
    ] {
        if keys.just_pressed(code) {
            queue.push(Command::Uplink { kind });
        }
    }

    // The pivots: p bridge, Shift+p ISP ops, g grid jump, x back out.
    if keys.just_pressed(KeyCode::KeyP) {
        let target = if SPRINT.iter().any(|c| keys.pressed(*c)) {
            PivotTarget::IspOps
        } else {
            PivotTarget::Bridge
        };
        queue.push(Command::Pivot { target });
    }
    if keys.just_pressed(KeyCode::KeyG) {
        queue.push(Command::Pivot {
            target: PivotTarget::GridJump,
        });
    }
    if keys.just_pressed(KeyCode::KeyX) {
        queue.push(Command::Unpivot);
    }
}

/// Pause, restart and quit. Returns `true` if the user asked to quit.
pub fn decode_session(keys: &ButtonInput<KeyCode>, queue: &mut CommandQueue) -> bool {
    // Session: Tab pause (pause moved off `p` to make room for the pivots),
    // R restart, Escape quit.
    if keys.just_pressed(KeyCode::Tab) {
        queue.paused = !queue.paused;
        let on = queue.paused;
        queue.push(Command::Pause { on });
    }
    if keys.just_pressed(KeyCode::KeyR) {
        queue.paused = false;
        queue.push(Command::Restart);
    }
    keys.just_pressed(KeyCode::Escape)
}

/// Press on the first alias down, release only when no alias is still held.
fn sync_hold(
    keys: &ButtonInput<KeyCode>,
    queue: &mut CommandQueue,
    codes: &[KeyCode],
    button: Button,
) {
    if codes.iter().any(|c| keys.just_pressed(*c)) {
        queue.push(Command::SetButton { button, down: true });
    } else if codes.iter().any(|c| keys.just_released(*c))
        && !codes.iter().any(|c| keys.pressed(*c))
    {
        queue.push(Command::SetButton {
            button,
            down: false,
        });
    }
}

/// The windowed input system: decode the keyboard into the command queue,
/// and toggle Net View on `N` (a screen switch, not a gameplay `Command`, so
/// it bypasses `CommandQueue` entirely).
///
/// With both seats on screen the keyboard is the infiltrator's and the mouse
/// the hacker's (see `seats`). With one, keys follow the seat being played:
/// the infiltrator's body only moves while you play the infiltrator, and `E`
/// while playing the hacker is the hacker's own (it opens the van deck — see
/// `screen`). `Esc` steps out of a screen rather than quitting while one is
/// open.
pub fn keyboard_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut queue: ResMut<CommandQueue>,
    mut exit: MessageWriter<AppExit>,
    mode: Res<State<AppMode>>,
    mut next_mode: ResMut<NextState<AppMode>>,
    seats: Option<Res<crate::seats::Seats>>,
    screen: Option<Res<crate::screen::Screen>>,
) {
    if keys.just_pressed(KeyCode::KeyN) {
        next_mode.set(match mode.get() {
            AppMode::GhostLobby => AppMode::NetView,
            AppMode::NetView => AppMode::GhostLobby,
        });
    }
    if crate::seats::keys_drive_infiltrator(seats.as_deref()) {
        decode_infiltrator(&keys, &mut queue);
    }
    decode_hacker(&keys, &mut queue);
    let quit = decode_session(&keys, &mut queue);
    let in_screen = screen
        .is_some_and(|s| s.infiltrator.phase != crate::screen::Phase::Closed || s.hacker.on());
    if quit && !in_screen {
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(setup: impl FnOnce(&mut ButtonInput<KeyCode>)) -> Vec<Command> {
        let mut keys = ButtonInput::<KeyCode>::default();
        setup(&mut keys);
        let mut queue = CommandQueue::default();
        decode(&keys, &mut queue);
        queue.pending
    }

    #[test]
    fn movement_keys_press_and_release_held_buttons() {
        assert_eq!(
            frame(|k| k.press(KeyCode::KeyA)),
            vec![Command::SetButton {
                button: Button::Left,
                down: true
            }]
        );
        // Release only fires when no alias is still held.
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyD);
        keys.press(KeyCode::ArrowRight);
        keys.clear();
        keys.release(KeyCode::KeyD);
        let mut queue = CommandQueue::default();
        decode(&keys, &mut queue);
        assert!(
            queue.pending.is_empty(),
            "ArrowRight still holds Right: {:?}",
            queue.pending
        );
        keys.clear();
        keys.release(KeyCode::ArrowRight);
        decode(&keys, &mut queue);
        assert_eq!(
            queue.pending,
            vec![Command::SetButton {
                button: Button::Right,
                down: false
            }]
        );
    }

    #[test]
    fn interact_is_both_a_hold_and_an_edge() {
        assert_eq!(
            frame(|k| k.press(KeyCode::KeyE)),
            vec![
                Command::SetButton {
                    button: Button::Interact,
                    down: true
                },
                Command::Interact,
            ]
        );
    }

    #[test]
    fn the_pivot_keys_map_to_the_footholds_they_name() {
        assert_eq!(
            frame(|k| k.press(KeyCode::KeyP)),
            vec![Command::Pivot {
                target: PivotTarget::Bridge
            }]
        );
        // Shift+p must reach the ISP — and shift itself also starts sprinting,
        // exactly as the TUI's shift arm does.
        assert_eq!(
            frame(|k| {
                k.press(KeyCode::ShiftLeft);
                k.press(KeyCode::KeyP);
            }),
            vec![
                Command::SetButton {
                    button: Button::Sprint,
                    down: true
                },
                Command::Pivot {
                    target: PivotTarget::IspOps
                },
            ]
        );
        assert_eq!(
            frame(|k| k.press(KeyCode::KeyG)),
            vec![Command::Pivot {
                target: PivotTarget::GridJump
            }]
        );
        assert_eq!(frame(|k| k.press(KeyCode::KeyX)), vec![Command::Unpivot]);
    }

    #[test]
    fn pause_toggles_and_restart_resumes() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Tab);
        let mut queue = CommandQueue::default();
        decode(&keys, &mut queue);
        assert_eq!(queue.pending, vec![Command::Pause { on: true }]);
        assert!(queue.paused);
        keys.clear();
        keys.release(KeyCode::Tab);
        keys.press(KeyCode::KeyR);
        queue.pending.clear();
        decode(&keys, &mut queue);
        assert_eq!(queue.pending, vec![Command::Restart]);
        assert!(!queue.paused, "restart clears the frontend pause latch");
    }

    #[test]
    fn uplinks_and_body_edges_fire_on_press() {
        assert_eq!(
            frame(|k| k.press(KeyCode::Digit1)),
            vec![Command::Uplink {
                kind: ActionKind::Camera
            }]
        );
        assert_eq!(frame(|k| k.press(KeyCode::Space)), vec![Command::Jump]);
        assert_eq!(frame(|k| k.press(KeyCode::KeyQ)), vec![Command::ThrowUsb]);
    }
}
