# Envelope 001 — Ghost Lobby

The first runnable Rust version of the Billy scenario: a single-floor,
asymmetric infiltration. An **infiltrator** moves through five rooms while a
**hacker** supports remotely; **Billy** — a low-level guard — comes in for a
snack, freezes, and then *interprets your behaviour* to decide what you were
after. The clean win is not "escape"; it is "leave with the real lead while
Billy tells his boss it was the shiny USB."

This document describes the ported model in `idaptik-core::scenario`. Every
numeric value cited here lives as a `pub const` in `constants.rs` (the single
source of truth) and is projected into the declarative `ScenarioDefinition` by
`ghost_lobby()`. Values are **not** cited against HTML line numbers; the
prototype is the origin, `constants.rs` is the authority.

## Shape (definition-as-data)

- `ScenarioDefinition` — rooms, doors, cameras, hide spots, props, objectives,
  difficulty presets, the full `TuningConstants` table, scoring weights, and the
  reset-RNG spawn ranges. Pure serde; JSON round-trips unchanged; `validate()`
  returns an Exchange-House-style report and `ok()` typed `ValidationError`s.
- `GhostLobbySim` — the deterministic simulation. `new(def, cfg, seed)` →
  `tick(&TickInput) -> Vec<Event>` → `snapshot()` / `restore()` / `debrief()`.
- Events are the spine; `log_view(event, tick, t) -> Option<LogLine>` renders the
  human log as a *view*.

## World

Five contiguous rooms tile `[20, 1260)`; the floor line is `y = 585`.

| Room | x | w | base support | ping bonus | sight × | lit |
|------|---|---|--------------|------------|---------|-----|
| kitchen (break room) | 20 | 250 | 1.00 | 0.00 | 1.00 | no |
| hall | 270 | 240 | 0.84 | 0.16 | 1.00 | no |
| office (USB trap) | 510 | 285 | 0.34 | 0.30 | 1.15 | **yes** |
| laundry (wardrobe) | 795 | 245 | 0.50 | 0.05 | 1.00 | no |
| exit (service exit) | 1040 | 220 | 0.69 | 0.12 | 1.00 | no |

Doors sit on the interior boundaries: `D1` @ 270, `D2` @ 510, `D3` @ 795,
`D4` @ 1040. A closed door blocks *everyone* (the infiltrator cannot open it,
the uplink cannot cross it for them). A routed hold opens the door after a
per-seed `route_delay`, then stays open `route_duration = 3.2 s`. Billy, blocked
at a door, badges through after `badge_delay` (difficulty-dependent), which sets
that door open for `1.55 s`.

Hide spots (crouch within radius): counter/kitchen@205 r58, copier/hall@404 r50,
desk/office@655 r62, wardrobe/laundry@885 r58, crate/exit@1125 r50.

Cameras sweep sinusoidally; the laundry feed is **stale** and never detects:
cam-hall/hall@330 range (292,495) phase 0.2; cam-office/office@565 range
(530,782) phase 1.3; cam-laundry/laundry@830 range (812,1015) phase 2.1 (stale).

## Difficulty table

| field | story | standard | operator |
|-------|-------|----------|----------|
| arrival window (s) | 24–31 | 19–26 | 15–21 |
| player_speed | 172 | 166 | 162 |
| sprint | 250 | 242 | 235 |
| billy_speed | 77 | 91 | 106 |
| billy_sight | 210 | 245 | 280 |
| support_limit | 7.2 | 5.2 | 3.8 |
| bandwidth_regen | 8.0 | 6.1 | 4.5 |
| badge_delay (s) | 1.65 | 1.32 | 1.02 |
| usb_timer (s) | 14 | 11 | 8.5 |
| camera_lock (s) | 1.05 | 0.78 | 0.58 |
| alert_gain | 0.78 | 1.0 | 1.2 |
| score_mult | 0.85 | 1.0 | 1.25 |
| rescue | yes | yes | **no** |

## RNG and reset

`Mulberry32` is an exact port of the prototype's `makeRng` (verified against
seed 123456: `next_u32 = [1642107918, 3424218114, 4280064779, 687244953]`). The
reset **draw order** is load-bearing: `note.x` = 92 + r·78, `usb.x` = 622 + r·112,
`arrival` = lerp(preset.arrival, r), `stale_pulse` = 2.5 + r·3.5 (drawn but
unused by the sim), then per-door `route_delay` = 0.22 + r·0.5 — and on
**Operator only**, each door draws one *additional* `r` (via a `&&`
short-circuit) and adds a 0.45 penalty when `floor(r·4) == i` — then
`snack_x` = 165 + r·68. Story/Standard consume 9 draws; Operator consumes 13.

Standard @ seed 123456 golden: `note.x = 121.82197739277035`,
`usb.x = 711.2934456393123`, `arrival = 25.97571166162379`,
`stale_pulse = 3.060040896642022`,
`door_delay = [0.3242859894223511, 0.33641854885965583, 0.45750674573238936,
0.5239999126922339]`, `snack_x = 186.3118865666911`.

## The tick pipeline (order is load-bearing)

`tick(&TickInput)` = one fixed `dt = 1/60` frame.

**PRE** (before `t += dt`, mirroring the prototype's synchronous keydown
handlers): P0 handles `Pause`/`Restart`; P2 applies the remaining immediates
(`Uplink`, `ForceCrisis`, `ForceExtract`, `ForceFail`) at the *pre-increment* `t`
so cooldown/throttle timestamps match; P3 returns early if the run has ended.

**UPDATE** — `t += dt`, `tick += 1`, then:

1. **timers** — decay camera ping / lights flicker / lockout / caught-grace /
   Billy stun; decrement action cooldowns; per door, decay `open` and count down
   `pending` → at ≤ 0, `open = 3.2` and emit `DoorHoldActive`; regenerate
   bandwidth `regen·(0.55 + 0.75·support)`; decay alert (0.45 quiet / 0.18
   crisis); track `max_alert`.
2. **player** — held movement, sprint (drains stamina), crouch (0.46× speed),
   jump (`vy = -430`), gravity 1080, landing noise, clamp `[26, 1250]`, door
   constraint; hidden = crouch + in-spot + `|vx| < 92` + grounded; all noise
   contributions `max()`'d, then a **single** `noise -= 1.1·dt`.
3. **interactions** — one priority-gated action: note-peel (kitchen, hold 0.85,
   seen-in-crisis teaches Billy + exposes the note) > USB-take (office, edge;
   in quiet this calls `begin_crisis(Usb)` **mid-pipeline**) > chute-enter
   (revealed, edge → extract) > chute-search (hold 1.55 → reveal) > pickpocket
   (Billy has the note + lights out, hold 0.72 → steal, player_interest = 100,
   Billy → Pursue). Then service-exit extract (`x ≥ 1242`) and USB throw.
   `secure_note`/`take_usb` test sight with the player's **new** position and
   Billy's **old** one.
4. **usb** — held-follow; thrown ballistics (`vy += 780·dt`,
   `vx *= 0.985^(dt·60)`, floor bounce −0.24 / friction 0.72, rest, clamp
   `[35, 1235]`); self-wipe timer → `wiped`, trace, `alert += 27·alert_gain`.
5. **vacuum** — if active: control loss past `x > 795` (rate 0.42, min 0.22,
   lag-warn once < 0.72) else regen 0.18; `x += 72·max(0.26, control)·dt`; in
   crisis with no belief and `|billy − vac| < 245`, distract Billy into
   Investigate; falls at `chute.x − 8` → reveal chute (vacuum).
6. **cameras** — suppressed entirely while `lockout > 0` or `flicker > 0`
   (detection bleeds `−2.2·dt`); else per non-stale same-room camera,
   `sweep = sin(t·0.75 + phase)·25`, band `[range0 + min(0,sweep),
   range1 + max(0,sweep)]`, seen if in band and not hidden and not
   (crouch & `|vx| < 80`); detection builds to `camera_lock` → reset, lockout
   5.5, `alert += 14·gain`, `player_interest += 30`, `CameraFlag`, Investigate.
7. **support** — target = room support (+ping bonus if pinging) − low-bandwidth
   penalty − `alert·0.0022` (+hidden 0.06) (+flicker 0.04), clamped
   `[0.05, 1]`; `support = approach(support, target, 1.15·dt)`. Then if crisis &
   support < 0.4 (post-approach) & not hidden: `isolation += dt·(1 + alert/130)`;
   `SupportFraying` at `support_limit·0.5` (throttled 4.2 s); **Partition** fail
   at `support_limit`; else isolation decays (2.2 hidden / 1.35 otherwise).
8. **behaviour (belief)** — player NEW pos, Billy OLD pos. When seen,
   player/note/usb interest grow by urgency (sprint > moving > still, + a carry
   bonus); when unseen they decay, but note interest is **latched** while
   exposed/carried and usb interest while thrown/carried. `belief = argmax` when
   `max(note, usb) ≥ 48`; announce-once → `BillyBeliefFormed`.
9. **billy (FSM)** — `stun > 0` short-circuits. Entering → move to snack →
   Shock(1.15) → Assess. Two pre-switch transitions **fall through** into the
   same-tick if-chain: `belief` (and not carrying) → Secure; `sees & dist < 115`
   → Pursue. Then the exclusive chain acts on the **mutated** mode. Secure grabs
   the object at `< 31` → Guard → CallBoss (`alert += 12·gain`, `BossCalled`) →
   Pursue. `BillyStateChanged` fires on **every** mode change.
10. **collisions** — crisis only; skipped when hidden / flicker / stun /
    caught-grace. Close = `|centre_dx| < 30` & same room. One-shot rescue if
    `rescue && support ≥ 0.68 && bandwidth ≥ 20` (costs 20 bw, stun 1.6, grace
    1.8, displace 58); otherwise **Caught**.
11. **objectives** — derive the note / misdirect / exit ledger; emit
    `ObjectivesUpdated` only when it changes.
12. **POST** — if still quiet and `t ≥ crisis_at`, `begin_crisis(Timer)` (Billy
    first moves the **next** tick — the asymmetry vs. USB crisis, which runs
    Billy the same tick). Then `alert ≥ 100` → **Lockdown**.

## Billy FSM

```
                 snack reached        timer
 Offsite ─crisis─▶ Entering ─────▶ Shock ────▶ Assess ◀───────┐
                                                 │  belief     │ give up
                                     belief──────┼──▶ Secure ──┤ (unseen,
                            sees&<115 ┌───────────┘     │ grab   │  ago>3.2)
                                      ▼                 ▼        │
 Investigate ◀── camera/vacuum ─── Pursue ◀── Guard ─▶ CallBoss ┘
     └────── reaches last-known ──▶ Assess
```

## Belief model

Billy never sees "the objective"; he infers it from your *urgency*. Growth is
sight-gated and urgency-weighted (sprint ≫ moving ≫ still), with a carry bonus
while you hold an item. Decay when unseen is latched off once the note is
exposed or the USB is in play, so a single visible mistake sticks. Belief forms
at interest ≥ 48 (argmax of note vs. usb) and is announced once. The misdirect
objective is "believed" once Billy has the USB, reports the USB, or
`usb_interest ≥ 72`.

## Scoring and grades

Extraction: base 700; note +650 / −260 (Billy has it) / −80 (none); misdirect
+330 (believed) / −180 (leak, reported note); boss +150 (none) / −140; cameras
+120 (zero flags) / −65 each; isolation +100 / −90 (if `max_isolation >
support_limit·0.55`); chute +110; usb-trace −170; rescue −80; time
`max(0, 220 − 1.7·t)`; `−2.7·max_alert`; `−28·failed_actions`. Final =
`max(0, js_round(raw · score_mult))`.

Failure: `(has_note ? 420 : 120) + (usb_believed ? 180 : 0) − 2·alert` (no
score multiplier, current alert).

Grades — success: S ≥ 1850, A ≥ 1450, B ≥ 1050, C ≥ 700, else D. Failure:
C ≥ 500, else D.

## Event catalog (log_view rendering)

`RunStarted`/`SeedAnnounced` (intro/seed); `PhaseChanged` (silent);
`CrisisBegan{Timer|Usb|Test}`; `UplinkAction` (telemetry); `CameraPinged`,
`DoorRouted`, `VacuumRouted`, `LightsFlickered`, `UplinkDenied`,
`DoorHoldActive`; `NoteSecured`, `NoteExposed`, `UsbTaken`, `UsbThrown`,
`UsbSelfWiped`, `BillyTookNote`, `BillyTookUsb`, `ChuteRevealed`,
`VacuumLagWarned`, `VacuumFell`, `CameraFlag`, `SupportFraying`,
`PickpocketSucceeded`, `BillyStateChanged` (silent), `BillyBeliefFormed`,
`BillyBadgedDoor`, `BossCalled`, `RescueUsed`, `ObjectivesUpdated` (silent);
`Extracted`, `MissionFailed`, `RunEnded`; `Paused`/`Resumed`/`Restarted`
(session). Throttled events: cooldown/bandwidth denials (1.2 s),
`SupportFraying` (4.2 s), `BillyBadgedDoor` (8 s).

## Teaching layer (frontend-only)

The simulation never explains itself to a new player: the opening log line says
what the rooms contain, not why anyone is there. `crates/idaptik-tui/src/tutorial.rs`
supplies the framing, as `LogLine`s on `Channel::Tutorial` — the channel the
determinism diff and the supervision fold already ignore (see
`scenario/vsm.rs`). It follows the loop the mission is meant to teach:

| beat | cue | trigger |
|------|-----|---------|
| purpose | the brief: the contact note is the prize, the office drive is the decoy, plus each objective's own label | run start, and again after a restart (the log is cleared) |
| opportunity | "Billy is inside and reading the room" | first `CrisisBegan` |
| response | which story Billy settled on, named in plain words | first `BillyBeliefFormed` |
| exploit | the misdirect is live / the misdirect is blown | first `ObjectivesUpdated` with a decided misdirect |
| retry | the run ended, in player terms, and how to restart | first `MissionFailed` |

Every cue is driven by an event the player can also read in the log, so the
tutorial never leaks hidden state: it points at evidence, it does not reveal
what Billy knows. Each cue fires at most once per run and the set re-arms on
`Restarted`.

The Bevy frontend does not use this module yet; it renders the same log lines
and would show the cues as soon as `app`-equivalent ingestion calls
`tutorial::brief` / `Tutor::observe`.

## Headless script API (`__IDAPTIK_TEST__` as data)

`idaptik-tui` runs scripts with no TTY:

```
idaptik-tui --headless --script FILE.json   # prints {event_log, debrief, final_snapshot}
idaptik-tui --replay FILE.json              # re-runs, PASS/FAIL determinism check
idaptik-tui --export definition|snapshot|debrief [--script FILE.json]
```

A script is a sparse, tick-indexed timeline:

```json
{
  "seed": 123456,
  "difficulty": "standard",
  "reduced_motion": false,
  "max_ticks": 400,
  "commands": [
    { "at": 0,   "hold": ["right"] },
    { "at": 30,  "test": { "hook": "force_crisis" } },
    { "at": 60,  "press": ["camera"] },
    { "at": 200, "test": { "hook": "force_extract", "method": "service_exit" } }
  ]
}
```

`hold` sets the held-button set (persists until the next line changes it);
`press` fires edge/uplink commands on that tick (`jump`, `interact`, `throw`,
`camera`/`door`/`vacuum`/`lights`, and the network verbs: `pivot`/`bridge`
for the maintenance bridge, `isp`/`ops` for the ISP operations host, `grid`
for the grid jump host, `unpivot` to back out one hop); `test` injects a Force* hook
(`force_crisis`, `force_extract` with `method` `service_exit`|`laundry_chute`,
`force_fail` with `reason` `caught`|`partition`|`lockdown`|`traced`).
