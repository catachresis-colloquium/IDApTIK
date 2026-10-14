//! Art direction from the storyboard bible, applied to the playable floor.
//! Render-only: nothing here is read by the simulation.
//!
//! * **Environment layers** (§5.4), back to front: sky gradient, skyline
//!   silhouette, lit window dots, rain, the building's roof slab with a
//!   signal-yellow fascia and stepped crown, the ground and grade line; per room
//!   a faint wash, a skirting bar, a cobalt cable tray and a light pool.
//! * **Props** (§5.3), painted, in place of the hide-spot bands: counter and
//!   fridge, copier, desk, staff locker, crate; a CRT on each room console;
//!   a ceiling light in every room and a badge reader by every door.
//! * **The hacker has a body** (§7.0): the sim seats the hacker in a van, so
//!   the van is parked on the service road with the hacker at a phosphor
//!   screen. (The bible's hacker-to-infiltrator "link" line is a storyboard
//!   annotation, not in-game UI — art rule 5 keeps floating UI off the
//!   infiltrator's world — so it is not drawn.)
//! * **Threat**: cameras with a pulsing red LED (their cones, and Billy's, are
//!   pixel-art overlays drawn by `cones`), and a red
//!   wash over the floor once the crisis starts.
//! * **Light and dark**: each room's fitting throws a stepped cone of light;
//!   a room whose lights are off (unlit, cut by the wiring, or flickering)
//!   goes dark under the people and the vision cones (`sync_room_light`).
//! * **Rain** outside the building, one-pixel streaks on the art grid; it
//!   holds still under reduced motion. (The old film grain and scanlines are
//!   gone: they fought the pixel grid.)
//!
//! Type is the one gap: the bible's Oswald / IBM Plex Mono / Inter are not
//! bundled yet, so text uses Bevy's default face.

use bevy::prelude::*;
use idaptik_core::netsim::graph::VantageKind;
use idaptik_core::scenario::common::Phase;
use idaptik_core::scenario::constants as c;
use idaptik_core::scenario::definition::CameraDef;

use crate::driver::SimState;
use crate::palette;
use crate::pixel;
use crate::scene::{CEILING_Y, GhostLobbySceneMarker, SceneMap};
use crate::sprites::{self, Actor, FigureImages, Painted, PaintedSprites, Pose};

// Z layers for the environment (the scene's own layers run 0–6).
const Z_SKY: f32 = -10.0;
const Z_SKYLINE: f32 = -9.0;
const Z_WINDOWS: f32 = -8.5;
const Z_SHELL: f32 = -1.0;
const Z_WASH: f32 = 0.2;
const Z_POOL: f32 = 0.3;
const Z_PROPS: f32 = 1.4;
const Z_VAN: f32 = 2.5;
/// Room darkness: over walls, props and doors, under people and cones.
const Z_SHADE: f32 = 3.4;

/// Props are drawn as the pixel art painted them, on the shared palette.
const PROP_TINT: Color = Color::WHITE;

/// How far either side of the building the street runs (canvas units).
const STREET: f64 = 230.0;
/// Where the van parks: past the building's right wall.
const VAN_GAP: f64 = 95.0;
/// Where the hacker sits in the van sprite's open side door (148 x 70 px):
/// the doorway spans x 68-97 and its sill is 10 px above the van's bottom, so
/// the seated figure (23 px wide) goes on the sill at x 80, leaving the
/// monitors on the right in view.
const VAN_SEAT_DX: f64 = (80.0 - 74.0) / sprites::SPRITE_PX as f64;
const VAN_SILL: f64 = 10.0 / sprites::SPRITE_PX as f64;

/// Marker: a camera mount's status LED.
#[derive(Component)]
pub struct CameraLed;

/// Where the camera sprite's lens is, in its own pixels from the top-left of
/// the right-facing sprite (15 x 16 px): inside the dark hood. The vision
/// cone starts here, so it leaves the lens whichever way the camera faces.
const CAMERA_LENS_PX: (f64, f64) = (10.5, 12.0);
/// The status LED, on the housing above the hood (same pixel frame).
const CAMERA_LED_PX: (f64, f64) = (8.5, 9.5);

/// Which way a camera faces: towards the middle of its sweep (+1 right, -1 left).
pub fn camera_facing(cam: &CameraDef) -> f64 {
    if (cam.range.0 + cam.range.1) * 0.5 < cam.x {
        -1.0
    } else {
        1.0
    }
}

/// A point on the camera sprite, given in the right-facing sprite's own
/// pixels, in canvas units (x right, y down). The sprite hangs centred on
/// `cam.x` from the ceiling and mirrors when the camera faces left, so the
/// point mirrors with it.
pub fn camera_point(cam: &CameraDef, sprite_w_px: f64, at_px: (f64, f64)) -> (f64, f64) {
    let px = f64::from(sprites::SPRITE_PX);
    let dx = (at_px.0 - sprite_w_px * 0.5) / px;
    (cam.x + camera_facing(cam) * dx, CEILING_Y + at_px.1 / px)
}

/// The camera's lens, where its vision cone starts.
pub fn camera_lens(cam: &CameraDef, sprite_w_px: f64) -> (f64, f64) {
    camera_point(cam, sprite_w_px, CAMERA_LENS_PX)
}

/// The parts of a room's lighting, by room index.
#[derive(Component, Clone, Copy)]
pub enum RoomLight {
    Fitting(usize),
    Cone(usize),
    Shade(usize),
}

/// How dark a room reads with its lights off (alpha of the darkness), and
/// when the wiring has cut them (darker still).
const UNLIT: f32 = 0.62;
const CUT: f32 = 0.78;

/// Per frame: each room's light. A room is lit when the level says so, the
/// wiring has not cut it, and no flicker has it off this instant.
pub fn sync_room_light(
    sim: Res<SimState>,
    time: Res<Time>,
    mut parts: Query<(&RoomLight, &mut Sprite)>,
) {
    let def = sim.sim.definition();
    let s = sim.sim.state();
    let still = sim.sim.config().reduced_motion;
    // A flicker blinks the lights at a few hertz (held off under reduced motion).
    let flick_off = s.lights_flicker > 0.0 && (still || (time.elapsed_secs() * 9.0).sin() > 0.0);
    for (part, mut sprite) in &mut parts {
        let k = match part {
            RoomLight::Fitting(k) | RoomLight::Cone(k) | RoomLight::Shade(k) => *k,
        };
        let Some(room) = def.rooms.get(k) else {
            continue;
        };
        let cut = sim.sim.signal_room_dark(room.id.as_str());
        let on = room.lit && !cut && !flick_off;
        sprite.color = match part {
            RoomLight::Fitting(_) => {
                if on {
                    Color::WHITE
                } else {
                    Color::srgb(0.55, 0.55, 0.6)
                }
            }
            RoomLight::Cone(_) => Color::WHITE.with_alpha(if on { 1.0 } else { 0.0 }),
            RoomLight::Shade(_) => palette::PITCH.with_alpha(if on {
                0.0
            } else if cut {
                CUT
            } else {
                UNLIT
            }),
        };
    }
}

/// Per frame: the hacker in the van types (the loop holds still under
/// reduced motion).
pub fn animate_hacker(
    sim: Res<SimState>,
    time: Res<Time>,
    mut poses: Query<(&Actor, &Pose, &mut Visibility)>,
) {
    let t = if sim.sim.config().reduced_motion {
        0.0
    } else {
        time.elapsed_secs()
    };
    let want = Pose::Type(sprites::type_frame(t));
    for (actor, pose, mut vis) in &mut poses {
        if *actor == Actor::Hacker {
            *vis = if *pose == want {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
}

/// One rain streak, by index (its fall is a pure function of the index and
/// the time, so nothing is stored per drop).
#[derive(Component)]
pub struct RainDrop(pub u32);

/// How many rain streaks fall at once.
const RAIN_DROPS: u32 = 180;
/// Where a streak starts falling, above the roofline.
const RAIN_TOP: f64 = CEILING_Y - 500.0;
/// A drop's fall speed (canvas units a second): a base plus up to the spread.
const RAIN_SPEED: (f64, f64) = (430.0, 220.0);
/// How long a splash shows where a drop hits the street, as a fraction of
/// its fall.
const RAIN_SPLASH: f64 = 0.035;

/// One rain streak at time `t`: where its bottom is (canvas units), how long
/// it is (art pixels), or a splash on the street. Every fall lands in a new
/// column at the drop's own speed, so the rain never repeats as a pattern.
#[derive(Debug, PartialEq)]
enum Rain {
    Streak { x: f64, y: f64, len: f32 },
    Splash { x: f64 },
}

fn rain_drop(i: u32, t: f64, floor: f64, span: f64) -> Rain {
    let unit = |h: u32| f64::from(h % 10_000) / 10_000.0;
    let speed = RAIN_SPEED.0 + RAIN_SPEED.1 * unit(hash(i.wrapping_mul(31) + 5));
    let fall = floor - RAIN_TOP;
    let s = t * speed / fall + unit(hash(i + 77));
    let cycle = s.floor() as u32;
    let column =
        |c: u32| unit(hash(i.wrapping_mul(7919) ^ c.wrapping_mul(0x9e37_79b9))) * span - STREET;
    let frac = s - s.floor();
    if frac < RAIN_SPLASH {
        return Rain::Splash {
            x: column(cycle.wrapping_sub(1)),
        };
    }
    // Faster drops streak longer (3 to 6 art pixels).
    let len = 3.0 + (3.0 * (speed - RAIN_SPEED.0) / RAIN_SPEED.1).round() as f32;
    Rain::Streak {
        x: column(cycle),
        y: RAIN_TOP + frac * fall,
        len,
    }
}

/// Marker: a room's crisis wash.
#[derive(Component)]
pub struct CrisisWash;

/// A cheap integer hash for repeatable noise (never touches the sim's RNG).
fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// A flat rectangle, centred.
fn quad(color: Color, size: Vec2, at: Vec2, z: f32) -> impl Bundle {
    (
        GhostLobbySceneMarker,
        Sprite::from_color(color, size),
        Transform::from_translation(at.extend(z)),
    )
}

/// A rectangle by canvas-space bottom-centre (props stand on the floor).
fn standing(
    map: &SceneMap,
    color: Color,
    x: f64,
    floor: f64,
    w: f32,
    h: f32,
    z: f32,
) -> impl Bundle {
    let base = map.point(x, floor);
    quad(color, Vec2::new(w, h), base + Vec2::new(0.0, h * 0.5), z)
}

/// Spawn every static layer. Runs after `scene::setup_scene`.
pub fn setup_art(
    mut commands: Commands,
    sim: Res<SimState>,
    figures: Res<FigureImages>,
    painted: Res<PaintedSprites>,
    backdrop: Res<crate::backdrop::BackdropArt>,
    mut images: ResMut<Assets<Image>>,
) {
    let def = sim.sim.definition();
    let map = SceneMap::new(def.world.width, def.world.floor, def.floor_id);
    let w = def.world.width;
    let floor = def.world.floor;
    let roof = CEILING_Y;
    // Wide enough that a camera following anyone never sees past the set.
    let full_w = (w + 2.0 * STREET + 2400.0) as f32;
    let centre_x = w * 0.5;

    // 1. Sky: a stepped gradient, SKY_TOP high to SKY_LOW at the ground.
    let sky_top = roof - 520.0;
    const BANDS: usize = 28;
    for i in 0..BANDS {
        let t = i as f32 / (BANDS - 1) as f32;
        let a = palette::SKY_TOP.to_srgba();
        let b = palette::SKY_LOW.to_srgba();
        let c = Color::srgb(
            a.red + (b.red - a.red) * t,
            a.green + (b.green - a.green) * t,
            a.blue + (b.blue - a.blue) * t,
        );
        let band_h = (floor - sky_top) / BANDS as f64;
        let y = sky_top + band_h * (i as f64 + 0.5);
        commands.spawn(quad(
            c,
            Vec2::new(full_w, band_h as f32 + 1.0),
            map.point(centre_x, y),
            Z_SKY,
        ));
    }

    // 2. Skyline: the two pixel-art strips alternating along the street,
    //    standing on the ground behind the building.
    let strips = [backdrop.get("skyline_a"), backdrop.get("skyline_b")];
    let mut x = -STREET - 900.0;
    let mut k = 0;
    while x < w + STREET + 900.0 {
        let strip = strips[k % 2];
        let half = f64::from(strip.size.x) * 0.5;
        commands.spawn((
            GhostLobbySceneMarker,
            sprites::painted_bundle(strip, map.point(x + half, floor), Z_SKYLINE, Color::WHITE),
        ));
        x += half * 2.0;
        k += 1;
    }

    // 3. The building's shell: walls, roof slab, signal fascia, stepped crown.
    let room_h = (floor - roof) as f32;
    commands.spawn(quad(
        palette::SLATE,
        Vec2::new(w as f32 + 24.0, room_h + 26.0),
        map.point(centre_x, (roof + floor) * 0.5 - 6.0),
        Z_SHELL,
    ));
    commands.spawn(quad(
        palette::SLAB,
        Vec2::new(w as f32 + 24.0, 18.0),
        map.point(centre_x, roof - 12.0),
        Z_SHELL + 0.1,
    ));
    commands.spawn(quad(
        palette::SIGNAL,
        Vec2::new(w as f32 + 24.0, 2.0),
        map.point(centre_x, roof - 3.0),
        Z_SHELL + 0.2,
    ));
    for (k, step_w) in [520.0f32, 380.0, 240.0].iter().enumerate() {
        let y = roof - 21.0 - 14.0 * k as f64 - 7.0;
        commands.spawn(quad(
            palette::SLAB,
            Vec2::new(*step_w, 14.0),
            map.point(centre_x, y),
            Z_SHELL + 0.1,
        ));
        commands.spawn(quad(
            palette::SIGNAL.with_alpha(0.7),
            Vec2::new(*step_w, 1.0),
            map.point(centre_x, y - 7.0),
            Z_SHELL + 0.2,
        ));
    }
    // A ring mast on the crown: this is a signals annex.
    commands.spawn(quad(
        palette::SLAB,
        Vec2::new(3.0, 70.0),
        map.point(centre_x, roof - 63.0 - 35.0),
        Z_SHELL,
    ));
    commands.spawn(quad(
        palette::ALARM,
        Vec2::new(5.0, 5.0),
        map.point(centre_x, roof - 63.0 - 72.0),
        Z_SHELL + 0.1,
    ));
    // The facade's lettering, in the pixel font, with a one-pixel shadow.
    let shadow = Some(pixel::rgba(palette::PITCH, 1.0));
    let name = pixel::text_canvas(
        "HOTEL OKTYABR / SIGNALS ANNEX",
        pixel::rgba(palette::SIGNAL, 1.0),
        shadow,
    )
    .into_painted(&mut images);
    commands.spawn((
        GhostLobbySceneMarker,
        sprites::painted_bundle(
            &name,
            map.point(centre_x, roof - 23.0),
            Z_SHELL + 0.3,
            Color::WHITE,
        ),
    ));
    let tag = pixel::text_canvas(
        "VG-233 / GHOST LOBBY",
        pixel::rgba(palette::BONE, 1.0),
        shadow,
    )
    .into_painted(&mut images);
    commands.spawn((
        GhostLobbySceneMarker,
        sprites::painted_bundle(
            &tag,
            map.point(centre_x, roof - 106.0),
            Z_SHELL + 0.3,
            Color::WHITE,
        ),
    ));

    // 4. Ground: soil below the walk line and a grade line across the street.
    commands.spawn(quad(
        palette::INK2,
        Vec2::new(full_w, 140.0),
        map.point(centre_x, floor + 74.0),
        Z_SHELL,
    ));
    commands.spawn(quad(
        palette::GRADE,
        Vec2::new(full_w, 2.0),
        map.point(centre_x, floor + 8.0),
        Z_SHELL + 0.1,
    ));
    for i in 0..90u32 {
        let x = f64::from(hash(i) % ((w + 2.0 * STREET) as u32)) - STREET;
        let y = floor + 16.0 + f64::from(hash(i + 999) % 120);
        commands.spawn(quad(
            palette::SLATE,
            Vec2::splat(2.0),
            map.point(x, y),
            Z_SHELL + 0.1,
        ));
    }

    // 5. Rooms: crisis wash, light fitting, light cone, darkness.
    for (k, room) in def.rooms.iter().enumerate() {
        let cx = room.x + room.w * 0.5;
        let size = Vec2::new(room.w as f32 - 6.0, room_h);
        let centre = map.box_center(room.x, roof, room.w, floor - roof);
        commands.spawn((
            quad(palette::ALARM.with_alpha(0.0), size, centre, Z_WASH + 0.05),
            CrisisWash,
        ));
        // The room's light fitting, hung under the ceiling, and its light:
        // a stepped cone down to the floor. Both follow the room's light
        // state each frame (`sync_room_light`).
        let fitting_y = roof + 2.0 + f64::from(painted.ceiling_light.size.y);
        commands.spawn((
            GhostLobbySceneMarker,
            RoomLight::Fitting(k),
            sprites::painted_bundle(
                &painted.ceiling_light,
                map.point(cx, fitting_y),
                Z_PROPS,
                Color::WHITE,
            ),
        ));
        let px = f64::from(sprites::SPRITE_PX);
        let top_w = (f64::from(painted.ceiling_light.size.x) * px) as usize;
        let bottom_w = ((room.w - 10.0) * px) as usize;
        let cone_h = ((floor - fitting_y) * px) as usize;
        let cone = pixel::light_cone(top_w, bottom_w, cone_h, palette::NEON, 0.16)
            .into_painted(&mut images);
        commands.spawn((
            GhostLobbySceneMarker,
            RoomLight::Cone(k),
            sprites::painted_bundle(&cone, map.point(cx, floor), Z_POOL, Color::WHITE),
        ));
        // Darkness over the whole room when its lights are off: over the
        // walls, props and doors, under the people and the vision cones.
        commands.spawn((
            quad(palette::PITCH.with_alpha(0.0), size, centre, Z_SHADE),
            RoomLight::Shade(k),
        ));
    }

    // 6. Props    }

    // 6. Props where the hide spots are, by what they are.
    for spot in &def.hide_spots {
        // The hide zone as a floor shadow its full width, under the prop.
        commands.spawn((
            GhostLobbySceneMarker,
            sprites::painted_bundle(
                &pixel::shadow_ellipse(
                    (spot.radius * 2.0 * f64::from(sprites::SPRITE_PX)) as usize,
                    8,
                    palette::PITCH,
                    0.45,
                )
                .into_painted(&mut images),
                map.point(spot.x, floor - 4.0 / f64::from(sprites::SPRITE_PX)),
                Z_PROPS - 0.05,
                Color::WHITE,
            ),
        ));
        spawn_prop(
            &mut commands,
            &map,
            &painted,
            spot.id.as_str(),
            spot.x,
            floor,
        );
    }

    // 6b. Consoles in rooms: a small desk, a CRT and its glow — a screen
    //     anyone can sit down at (`E`).
    for con in &def.consoles {
        if let idaptik_core::scenario::definition::ConsolePlace::Room { x, .. } = con.place {
            commands.spawn((
                GhostLobbySceneMarker,
                sprites::painted_bundle(
                    &painted.crt_console,
                    map.point(x, floor),
                    Z_PROPS,
                    PROP_TINT,
                ),
            ));
            commands.spawn((
                GhostLobbySceneMarker,
                Sprite {
                    image: figures.disc.clone(),
                    color: palette::PHOSPHOR.with_alpha(0.07),
                    custom_size: Some(Vec2::new(110.0, 80.0)),
                    ..Default::default()
                },
                Transform::from_translation(map.point(x, floor - 48.0).extend(Z_POOL)),
            ));
        }
    }

    // 7. Cameras: the camera sprite hung from the ceiling, facing the middle
    //    of its sweep, with a pulsing red LED on the housing.
    for cam in &def.cameras {
        let facing = camera_facing(cam) as f32;
        let size = painted.camera.size;
        let led = camera_point(cam, f64::from(size.x * sprites::SPRITE_PX), CAMERA_LED_PX);
        let top = map.point(cam.x, roof);
        commands.spawn((
            GhostLobbySceneMarker,
            Sprite {
                image: painted.camera.image.clone(),
                custom_size: Some(size),
                flip_x: facing < 0.0,
                ..Default::default()
            },
            Transform::from_translation((top - Vec2::new(0.0, size.y * 0.5)).extend(Z_PROPS)),
        ));
        commands.spawn((
            quad(
                palette::ALARM,
                Vec2::splat(1.0 / sprites::SPRITE_PX),
                map.point(led.0, led.1),
                Z_PROPS + 0.2,
            ),
            CameraLed,
        ));
    }

    // 7b. A badge reader beside every door, at hand height on the near side.
    for door in &def.doors {
        commands.spawn((
            GhostLobbySceneMarker,
            sprites::painted_bundle(
                &painted.badge_reader,
                map.point(door.x - 16.0, floor - 40.0),
                Z_PROPS,
                Color::WHITE,
            ),
        ));
    }

    // 8. The hacker's body in the world: the van on the service road.
    let vantage = sim.sim.state().agents.hacker.vantage().kind;
    if vantage == VantageKind::Van {
        spawn_van(&mut commands, &map, &painted, w + VAN_GAP, floor);
    }

    // 8a. Outside: the rope down the front facade, and the roof's hatches.
    let px = 1.0 / sprites::SPRITE_PX;
    if let Some(ext) = &sim.sim.definition().exterior {
        let rope = ext.rope;
        // Tied off on the roof, over the parapet, and down to the pavement.
        let top = map.point(rope.foot_x, ext.roof_y);
        let foot = map.point(rope.foot_x, floor);
        commands.spawn(quad(
            palette::DUST,
            Vec2::new(px, top.y - foot.y),
            (top + foot) * 0.5,
            Z_PROPS,
        ));
        commands.spawn(quad(
            palette::DUST,
            Vec2::new((rope.top_x - rope.foot_x) as f32, px),
            map.point((rope.top_x + rope.foot_x) * 0.5, ext.roof_y - 2.0),
            Z_PROPS,
        ));
        commands.spawn(quad(
            palette::SLAB,
            Vec2::new(5.0 * px, 4.0 * px),
            map.point(rope.top_x, ext.roof_y - 2.0 * px as f64),
            Z_PROPS + 0.01,
        ));
        // The parapet at the far end: what stops you there.
        let right = ext.roof.1 + sim.sim.definition().player.w;
        let (pw, ph) = (c::PARAPET_W, c::PARAPET_H);
        let at = map.point(right + pw * 0.5, ext.roof_y - ph * 0.5);
        commands.spawn(quad(
            palette::SLAB,
            Vec2::new(pw as f32, ph as f32),
            at,
            Z_PROPS,
        ));
        commands.spawn(quad(
            palette::SIGNAL,
            Vec2::new(pw as f32, px),
            map.point(right + pw * 0.5, ext.roof_y - ph),
            Z_PROPS + 0.01,
        ));
        for hatch in &ext.hatches {
            // A steel hatch, a pixel proud of the slab, its handle catching the light.
            commands.spawn(quad(
                palette::SLATE,
                Vec2::new(24.0, 3.0 * px),
                map.point(hatch.x, ext.roof_y - 1.5 * px as f64),
                Z_PROPS,
            ));
            commands.spawn(quad(
                palette::DUST,
                Vec2::new(4.0 * px, px),
                map.point(hatch.x, ext.roof_y - 3.5 * px as f64),
                Z_PROPS + 0.01,
            ));
        }
    }

    // 8b. Rain: one-art-pixel streaks, placed each frame by `animate_art`.
    for i in 0..RAIN_DROPS {
        commands.spawn((
            GhostLobbySceneMarker,
            RainDrop(i),
            // One art pixel, stretched into a streak each frame; some drops
            // fainter (further off) than others.
            Sprite::from_color(
                palette::RAIN.with_alpha(0.2 + 0.25 * (hash(i + 5) % 100) as f32 / 100.0),
                Vec2::splat(px),
            ),
            Transform::from_translation(Vec3::new(0.0, 0.0, Z_WINDOWS + 0.2)),
            Visibility::Hidden,
        ));
    }
}

/// One floor prop, standing at canvas `x` on the walk line.
fn spawn_prop(
    commands: &mut Commands,
    map: &SceneMap,
    art: &PaintedSprites,
    id: &str,
    x: f64,
    floor: f64,
) {
    // Painted pieces and their offsets along the floor.
    let pieces: Vec<(&Painted, f64)> = match id {
        // The fridge stands on the counter's left, away from the doorway.
        "counter" => {
            let gap = f64::from(art.counter.size.x + art.fridge.size.x) * 0.5 + 2.0;
            vec![(&art.counter, 0.0), (&art.fridge, -gap)]
        }
        "copier" => vec![(&art.copier, 0.0)],
        "desk" => vec![(&art.desk, 0.0)],
        "wardrobe" => vec![(&art.locker, 0.0)],
        "crate" => vec![(&art.crate_, 0.0)],
        _ => {
            commands.spawn(standing(map, palette::SLAB, x, floor, 40.0, 40.0, Z_PROPS));
            return;
        }
    };
    for (piece, dx) in pieces {
        commands.spawn((
            GhostLobbySceneMarker,
            sprites::painted_bundle(piece, map.point(x + dx, floor), Z_PROPS, PROP_TINT),
        ));
    }
}

/// The van: the painted van on the service road, its side door open on the
/// hacker seated at the phosphor monitors inside (bible §5.3 `van`, §7.0 posture `van`).
fn spawn_van(commands: &mut Commands, map: &SceneMap, art: &PaintedSprites, x: f64, floor: f64) {
    let z = Z_VAN;
    commands.spawn((
        GhostLobbySceneMarker,
        sprites::painted_bundle(&art.van, map.point(x, floor), z, PROP_TINT),
    ));
    commands
        .spawn((
            GhostLobbySceneMarker,
            // Seated on the sill of the open side door, facing the monitors.
            Transform::from_translation(
                map.point(x + VAN_SEAT_DX, floor - VAN_SILL).extend(z + 0.2),
            ),
            Visibility::Inherited,
        ))
        .with_children(|p| {
            sprites::spawn_person(p, art, Actor::Hacker, &Pose::all_for(Actor::Hacker));
        });
}

/// Where the hacker sits in the van, in canvas units — what the camera
/// follows while you play the hacker.
pub fn van_anchor(world_width: f64, floor: f64) -> (f64, f64) {
    (world_width + VAN_GAP, floor - 40.0)
}

/// Per frame: pulse the camera LEDs, wash the floor red in crisis, and draw
/// the gizmo layers (rain, cable trays, Billy's cone).
#[allow(clippy::too_many_arguments)]
pub fn animate_art(
    sim: Res<SimState>,
    map: Res<SceneMap>,
    time: Res<Time>,
    mut leds: Query<&mut Sprite, (With<CameraLed>, Without<CrisisWash>)>,
    mut washes: Query<&mut Sprite, (With<CrisisWash>, Without<CameraLed>)>,
    mut rain: Query<(&RainDrop, &mut Transform, &mut Visibility)>,
) {
    let def = sim.sim.definition();
    let s = sim.sim.state();
    let still = sim.sim.config().reduced_motion;
    let t = if still { 0.0 } else { time.elapsed_secs() };

    let pulse = 0.35 + 0.65 * (0.5 + 0.5 * (t * std::f32::consts::TAU / 2.4).sin());
    let dead_floor = s.camera_lockout > 0.0 || s.lights_flicker > 0.0;
    for mut led in &mut leds {
        led.color = palette::ALARM.with_alpha(if dead_floor { 0.15 } else { pulse });
    }
    let crisis = if s.phase == Phase::Crisis {
        0.035 + 0.025 * (0.5 + 0.5 * (t * 3.0).sin())
    } else {
        0.0
    };
    for mut wash in &mut washes {
        wash.color = palette::ALARM.with_alpha(crisis);
    }

    let floor = def.world.floor;
    let w = def.world.width;

    // Rain, outside the building only: one-pixel streaks on the art grid.
    let px = 1.0 / f64::from(sprites::SPRITE_PX);
    let snap = |v: f64| (v / px).round() * px;
    let span = w + 2.0 * STREET;
    for (drop, mut tf, mut vis) in &mut rain {
        let (x, bottom, size) = match rain_drop(drop.0, f64::from(t), floor, span) {
            Rain::Streak { x, y, len } => (x, y, Vec2::new(1.0, len)),
            Rain::Splash { x } => (x, floor, Vec2::new(3.0, 1.0)),
        };
        let (x, bottom) = (snap(x), snap(bottom));
        let over_building = x > -12.0 && x < w + 12.0;
        let shown = if over_building {
            bottom < CEILING_Y - 22.0
        } else {
            bottom <= floor
        };
        *vis = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        // The sprite is one art pixel: scale it to the streak, bottom on the grid.
        let at = map.point(x, bottom - f64::from(size.y) * px * 0.5);
        tf.translation.x = at.x;
        tf.translation.y = at.y;
        tf.scale = size.extend(1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rain_lands_in_a_new_column_each_fall_and_splashes() {
        let (floor, span) = (600.0, 2000.0);
        let mut columns = std::collections::HashSet::new();
        let mut splashes = 0;
        for k in 0..400 {
            match rain_drop(3, f64::from(k) * 0.05, floor, span) {
                Rain::Streak { x, y, len } => {
                    columns.insert(x.to_bits());
                    assert!((RAIN_TOP..=floor).contains(&y));
                    assert!((3.0..=6.0).contains(&len));
                }
                Rain::Splash { .. } => splashes += 1,
            }
        }
        // Twenty seconds of one drop: many falls, each in its own column.
        assert!(columns.len() > 10, "{} columns", columns.len());
        assert!(splashes > 0);
        // Drops fall at different speeds.
        let y = |i| match rain_drop(i, 0.4, floor, span) {
            Rain::Streak { y, .. } => y.to_bits(),
            Rain::Splash { .. } => 0,
        };
        assert!(
            (0..20)
                .map(y)
                .collect::<std::collections::HashSet<_>>()
                .len()
                > 15
        );
    }

    #[test]
    fn the_cone_leaves_the_lens_whichever_way_the_camera_faces() {
        let cam = |lo, hi| CameraDef {
            id: "c".into(),
            room: "r".into(),
            x: 400.0,
            range: (lo, hi),
            phase: 0.0,
            stale: false,
        };
        let (right, left) = (cam(420.0, 600.0), cam(200.0, 380.0));
        assert_eq!(camera_facing(&right), 1.0);
        assert_eq!(camera_facing(&left), -1.0);
        let px = f64::from(sprites::SPRITE_PX);
        let (r, l) = (camera_lens(&right, 15.0), camera_lens(&left, 15.0));
        // Three art pixels right of the sprite's centre column, facing right...
        assert!((r.0 - (400.0 + 3.0 / px)).abs() < 1e-9);
        // ...and the mirror image facing left, at the same height.
        assert!((l.0 - (400.0 - 3.0 / px)).abs() < 1e-9);
        assert_eq!(r.1, l.1);
    }
}
