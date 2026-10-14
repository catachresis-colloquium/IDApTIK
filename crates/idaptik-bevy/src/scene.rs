//! The side-on 2.5D cross-section of the building, Gunpoint-like and very
//! basic: flat-shaded quads for readability over art.
//!
//! The scenario's coordinates are canvas-style (x rightwards, y *downwards*,
//! the floor line at `world.floor`); [`SceneMap`] converts them into Bevy's
//! y-up scene space. Rooms are laid out data-driven from the room definitions
//! as one horizontal row per floor; [`SceneMap::row_lift`] stacks additional
//! floor rows vertically, so the coming multi-floor Exchange House building
//! runtime slots in as extra rows without touching the mapping.

use bevy::camera::ScalingMode;
use bevy::prelude::*;
use idaptik_core::interp::VisualSlot;
use idaptik_core::scenario::common::BillyMode;

use crate::driver::{SimState, VisualBuffers};
use crate::palette;
use crate::sprites;

/// How far through the current simulation tick this frame is drawn.
///
/// `0.0` is the tick that just completed, `1.0` the one about to. Bevy keeps
/// the leftover time that has not yet accumulated into a fixed step, which is
/// exactly the fraction we want; `_f64` because the simulation is `f64`
/// throughout and rounding to `f32` here would reintroduce the judder we are
/// removing.
///
/// This is the only place render timing enters the scene, and it is the seam to
/// cut when this lifts into a standalone engine — see `ENGINE_EXTRACTION_NOTES.md`.
fn render_alpha(fixed: &Time<Fixed>) -> f64 {
    fixed.overstep_fraction_f64()
}

/// Room height, floor to ceiling (canvas units). Render-only: the sim has no
/// ceiling. About 2.8 people tall (the infiltrator is 54), which a jump (head
/// about 140 above the floor at the apex) never reaches.
pub const ROOM_H: f64 = 150.0;
/// World-y (canvas, downwards) of the room ceiling line for a floor row.
pub const CEILING_Y: f64 = idaptik_core::scenario::constants::FLOOR - ROOM_H;
/// Vertical scene-space gap between stacked floor rows (future multi-floor).
pub const ROW_STACK: f32 = 440.0;
/// How far the whole cross-section is shifted down so it centres on screen.
const VIEW_LIFT: f32 = 250.0;
/// The floor camera's base zoom, in world units per screen pixel: three
/// screen pixels per art pixel, so the pixel art stays square and even. Every
/// shot scale multiplies this (the hacker's 0.6 gives five per art pixel).
pub const PIXEL_SCALE: f32 = 1.0 / (3.0 * sprites::SPRITE_PX);
/// Door slab size: the door sprite's own (16 x 71 px at [`sprites::SPRITE_PX`]).
const DOOR_W: f32 = 16.0 / sprites::SPRITE_PX;
const DOOR_H: f32 = 71.0 / sprites::SPRITE_PX;
/// Door state is a tint over the steel slab (white leaves it as drawn).
const DOOR_SHUT: Color = Color::WHITE;
const DOOR_ROUTING: Color = Color::srgb(1.0, 0.92, 0.62);
const DOOR_OPEN: Color = Color::srgb(0.72, 0.82, 1.0);
const DOOR_WIRE_LOCKED: Color = Color::srgb(1.0, 0.52, 0.52);

// Z layering: rooms behind everything, actors in front.
const Z_ROOM: f32 = 0.0;
const Z_FLOOR: f32 = 1.0;
const Z_DOOR: f32 = 2.0;
const Z_PROP: f32 = 3.0;
const Z_ACTOR: f32 = 4.0;

/// Canvas-space → scene-space mapping for one floor row.
#[derive(Resource, Clone, Copy)]
pub struct SceneMap {
    width: f32,
    floor: f32,
    row: u32,
}

impl SceneMap {
    /// Build the mapping for floor row `row` of a world `width` wide with its
    /// walk line at canvas-y `floor`.
    pub fn new(width: f64, floor: f64, row: u32) -> Self {
        Self {
            width: width as f32,
            floor: floor as f32,
            row,
        }
    }

    /// The extra scene-space height this floor row is lifted by.
    fn row_lift(&self) -> f32 {
        self.row as f32 * ROW_STACK
    }

    /// A canvas-space point (y downwards) in scene space (y upwards).
    pub fn point(&self, x: f64, y: f64) -> Vec2 {
        Vec2::new(
            x as f32 - self.width * 0.5,
            (self.floor - y as f32) - VIEW_LIFT + self.row_lift(),
        )
    }

    /// Centre of a canvas-space box given its top-left corner and size.
    pub fn box_center(&self, x: f64, y_top: f64, w: f64, h: f64) -> Vec2 {
        self.point(x + w * 0.5, y_top + h * 0.5)
    }
}

/// Marker: the infiltrator's body quad.
#[derive(Component)]
pub struct PlayerMarker;
/// Marker: Billy's body quad.
#[derive(Component)]
pub struct BillyMarker;
/// Marker: a door slab, by definition index.
#[derive(Component)]
pub struct DoorMarker(pub usize);
/// Marker: the contact note.
#[derive(Component)]
pub struct NoteMarker;
/// Marker: the USB trap.
#[derive(Component)]
pub struct UsbMarker;
/// Marker: the laundry chute.
#[derive(Component)]
pub struct ChuteMarker;
/// Marker: the robot vacuum.
#[derive(Component)]
pub struct VacuumMarker;
/// Marker: every entity the Ghost Lobby scene spawns except the camera --
/// shown/hidden as a block when Net View toggles on/off, so the two screens
/// never render on top of each other.
#[derive(Component)]
pub struct GhostLobbySceneMarker;

/// Spawn the whole static scene from the scenario definition, plus the dynamic
/// entities the sync systems drive. Everything is data-driven: rooms, doors,
/// cameras, hide spots and props come from the definition, never from names.
/// Colours come from [`palette`]; the environment layers, props and grain live
/// in [`crate::art`].
pub fn setup_scene(mut commands: Commands, sim: Res<SimState>, mut images: ResMut<Assets<Image>>) {
    let def = sim.sim.definition();
    let map = SceneMap::new(def.world.width, def.world.floor, def.floor_id);
    let floor = def.world.floor;
    let room_h = floor - CEILING_Y;
    let figures = sprites::FigureImages::new(&mut images);
    let painted = sprites::PaintedSprites::new(&mut images);

    // One camera per seat, the hacker's deck layer and the UI camera;
    // `seats::layout_seats` gives each its pane and `follow_camera` moves them.
    crate::seats::spawn_cameras(&mut commands, PIXEL_SCALE);
    commands.insert_resource(ClearColor(palette::PITCH));

    // The floor the whole row stands on: worn linoleum in 24 px lengths.
    let floor_tile = crate::pixel::floor_tile(24, 7, palette::SLATE, palette::DUST, palette::INK2)
        .into_painted(&mut images);
    commands.spawn((
        GhostLobbySceneMarker,
        Sprite {
            image: floor_tile.image.clone(),
            custom_size: Some(Vec2::new(map.width + 8.0, floor_tile.size.y)),
            image_mode: SpriteImageMode::Tiled {
                tile_x: true,
                tile_y: false,
                stretch_value: 1.0 / sprites::SPRITE_PX,
            },
            ..Default::default()
        },
        Transform::from_translation(
            map.point(
                def.world.width * 0.5,
                floor + f64::from(floor_tile.size.y) * 0.5,
            )
            .extend(Z_FLOOR),
        ),
    ));

    // Rooms: a row of side-view boxes, one per definition (bible §5.4, layer 8).
    for room in &def.rooms {
        let fill = if room.lit {
            palette::ROOM_FILL_LIT
        } else {
            palette::ROOM_FILL
        };
        commands.spawn((
            GhostLobbySceneMarker,
            Sprite::from_color(fill, Vec2::new(room.w as f32 - 6.0, room_h as f32)),
            Transform::from_translation(
                map.box_center(room.x, CEILING_Y, room.w, room_h)
                    .extend(Z_ROOM),
            ),
        ));
    }

    // Door slabs (positions/colours are synced from state every frame).
    // Each door hangs in a steel frame: jambs and a head round the opening,
    // in front of the slab so a sliding door disappears up into the wall.
    let frame =
        crate::pixel::door_frame(16, 71, 2, palette::SLAB, palette::DUST).into_painted(&mut images);
    for (i, door) in def.doors.iter().enumerate() {
        commands.spawn((
            GhostLobbySceneMarker,
            sprites::painted_bundle(&frame, map.point(door.x, floor), Z_DOOR + 0.1, Color::WHITE),
        ));
        commands.spawn((
            GhostLobbySceneMarker,
            DoorMarker(i),
            Sprite {
                image: painted.door.image.clone(),
                custom_size: Some(Vec2::new(DOOR_W, DOOR_H)),
                ..Default::default()
            },
            Transform::from_translation(Vec3::new(0.0, 0.0, Z_DOOR)),
        ));
    }

    // The extraction edge: a fire exit door at the right end of the row.
    commands.spawn((
        GhostLobbySceneMarker,
        sprites::painted_bundle(
            &painted.exit_door,
            map.point(
                def.world.width - f64::from(painted.exit_door.size.x) * 0.5 - 2.0,
                floor,
            ),
            Z_DOOR,
            Color::WHITE,
        ),
    ));

    // Pickups and devices. Each sprite is centred where the sim puts it; the
    // sync system moves and tints it.
    let centred = |p: &sprites::Painted| Sprite {
        image: p.image.clone(),
        custom_size: Some(p.size),
        ..Default::default()
    };
    commands.spawn((
        GhostLobbySceneMarker,
        NoteMarker,
        centred(&painted.note),
        Transform::from_translation(Vec3::new(0.0, 0.0, Z_PROP)),
    ));
    commands.spawn((
        GhostLobbySceneMarker,
        UsbMarker,
        centred(&painted.usb),
        Transform::from_translation(Vec3::new(0.0, 0.0, Z_PROP)),
    ));
    // The laundry chute stands on the floor at its x, its hatch up the wall.
    commands.spawn((
        GhostLobbySceneMarker,
        ChuteMarker,
        sprites::painted_bundle(
            &painted.chute,
            map.point(def.props.chute.x, floor),
            Z_PROP,
            Color::WHITE,
        ),
    ));
    commands.spawn((
        GhostLobbySceneMarker,
        VacuumMarker,
        centred(&painted.vacuum),
        Transform::from_translation(Vec3::new(0.0, 0.0, Z_PROP)),
    ));

    // The infiltrator and Billy: invisible body boxes the sync systems place,
    // carrying the painted figures (feet at the box bottom).
    commands
        .spawn((
            GhostLobbySceneMarker,
            PlayerMarker,
            Sprite::from_color(
                Color::NONE,
                Vec2::new(def.player.w as f32, def.player.h as f32),
            ),
            Transform::from_translation(Vec3::new(0.0, 0.0, Z_ACTOR)),
        ))
        .with_children(|parent| {
            sprites::spawn_person(
                parent,
                &painted,
                sprites::Actor::Infiltrator,
                &sprites::Pose::all_for(sprites::Actor::Infiltrator),
            );
        });
    commands
        .spawn((
            GhostLobbySceneMarker,
            BillyMarker,
            Sprite::from_color(
                Color::NONE,
                Vec2::new(def.billy.w as f32, def.billy.h as f32),
            ),
            Transform::from_translation(Vec3::new(0.0, 0.0, Z_ACTOR)),
        ))
        .with_children(|parent| {
            sprites::spawn_person(
                parent,
                &painted,
                sprites::Actor::Guard,
                &sprites::Pose::all_for(sprites::Actor::Guard),
            );
        });

    commands.insert_resource(figures);
    commands.insert_resource(painted);
    commands.insert_resource(map);
}

/// Door slabs: colour says shut / routing / open / locked by a wire, and an
/// opening door slides up out of the walk line, Gunpoint-style. Jamb colours
/// follow the bible (§7.3): shut doors oxblood, open ones verdigris.
pub fn sync_doors(
    sim: Res<SimState>,
    map: Res<SceneMap>,
    visual: Res<VisualBuffers>,
    fixed: Res<Time<Fixed>>,
    mut doors: Query<(&DoorMarker, &mut Transform, &mut Sprite)>,
) {
    let s = sim.sim.state();
    let alpha = render_alpha(&fixed);
    let def = sim.sim.definition();
    for (marker, mut tf, mut sprite) in &mut doors {
        let Some(door) = s.doors.get(marker.0) else {
            continue;
        };
        // The slab slides open over several ticks, so its openness interpolates
        // like a position. Colour keys off the live tick.
        let open = visual.doors.sample(marker.0, alpha).clamp(0.0, 1.0) as f32;
        let base = map.point(door.x, def.world.floor - f64::from(DOOR_H) * 0.5);
        tf.translation.x = base.x;
        tf.translation.y = base.y + open * DOOR_H * 0.92;
        let node = idaptik_core::scenario::floor_graph::door_node_id(door.id.clone());
        let wire_locked = match (&def.signal, &s.signal) {
            (Some(net), Some(st)) => net
                .devices
                .iter()
                .zip(&st.devices)
                .any(|(d, ds)| d.node.as_deref() == Some(node.as_str()) && ds.locked),
            _ => false,
        };
        // The front door is locked until picked or hacked.
        let front_locked = def
            .exterior
            .as_ref()
            .is_some_and(|e| e.front_door == door.id && !door.unlocked);
        sprite.color = if wire_locked || (front_locked && open <= 0.05) {
            DOOR_WIRE_LOCKED
        } else if open > 0.05 {
            DOOR_OPEN
        } else if door.pending > 0.0 {
            DOOR_ROUTING
        } else {
            DOOR_SHUT
        };
    }
}

/// An actor's canvas-space body box (top-left corner plus size).
struct BodyBox {
    x: f64,
    y_top: f64,
    w: f64,
    h: f64,
}

/// Position/size an actor's body quad from canvas-space state.
/// `crouch_scale` shrinks the drawn height while keeping the feet planted.
fn place_body(
    map: &SceneMap,
    tf: &mut Transform,
    sprite: &mut Sprite,
    body: BodyBox,
    crouch_scale: f64,
) {
    let dh = body.h * crouch_scale;
    let bottom = body.y_top + body.h;
    let center = map.box_center(body.x, bottom - dh, body.w, dh);
    tf.translation.x = center.x;
    tf.translation.y = center.y;
    sprite.custom_size = Some(Vec2::new(body.w as f32, dh as f32));
}

/// Render-only: how far to shift a figure drawn at canvas centre `cx` (half
/// its drawn width `half`) so it stands clear of every closed door. The sim
/// stops a body when its *centre* is 3 units short of a closed door's plane
/// (ported behaviour, pinned by the goldens), which would otherwise draw half
/// the figure inside the door.
pub fn door_clearance(
    cx: f64,
    half: f64,
    doors: &[idaptik_core::scenario::state::DoorState],
) -> f64 {
    let reach = half + f64::from(DOOR_W) * 0.5;
    let mut dx = 0.0;
    for door in doors.iter().filter(|d| d.open <= 0.0 && !d.unlocked) {
        let gap = cx + dx - door.x;
        if gap.abs() < reach {
            dx += gap.signum() * (reach - gap.abs());
        }
    }
    dx
}

/// The infiltrator: places the body box, then poses, faces and fades the
/// figure on it. Position interpolates; every discrete flag is read live.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn sync_player(
    sim: Res<SimState>,
    map: Res<SceneMap>,
    visual: Res<VisualBuffers>,
    fixed: Res<Time<Fixed>>,
    mut body: Query<(&mut Transform, &mut Sprite), With<PlayerMarker>>,
    mut roots: Query<
        (&sprites::Actor, &mut Transform),
        (With<sprites::FigureRoot>, Without<PlayerMarker>),
    >,
    mut poses: Query<(&sprites::Actor, &sprites::Pose, &mut Visibility)>,
    mut parts: Query<(&sprites::Actor, &sprites::FigurePart, &mut Sprite), Without<PlayerMarker>>,
    art: Res<sprites::PaintedSprites>,
    time: Res<Time>,
) {
    let def = sim.sim.definition();
    let p = &sim.sim.state().player;
    let pose = visual
        .poses
        .sample(VisualSlot::Player.index(), render_alpha(&fixed));
    let Ok((mut tf, mut sprite)) = body.single_mut() else {
        return;
    };
    let crouch = if p.crouching { 0.62 } else { 1.0 };
    place_body(
        &map,
        &mut tf,
        &mut sprite,
        BodyBox {
            x: pose.x,
            y_top: pose.y,
            w: def.player.w,
            h: def.player.h,
        },
        crouch,
    );
    sprite.color = Color::NONE;
    // Sprites are drawn at their true size: no figure scaling.
    let scale = 1.0;
    let feet = -(def.player.h * crouch) as f32 * 0.5;
    let facing = if pose.facing < 0.0 { -1.0 } else { 1.0 };
    for (actor, mut root) in &mut roots {
        if *actor == sprites::Actor::Infiltrator {
            root.translation.y = feet;
            root.scale = Vec3::new(facing * scale, scale, 1.0);
        }
    }
    // Walking steps through the cycle by distance; in the air, or still, the
    // figure stands (or crouches).
    let gait = if p.grounded {
        let stride = if p.crouching {
            sprites::SNEAK_STRIDE
        } else {
            sprites::STRIDE
        };
        sprites::gait_frame(pose.x, p.vx, stride)
    } else {
        None
    };
    let s = sim.sim.state();
    let want = match (p.crouching, gait) {
        // Seated at a console: the typing loop.
        _ if s.using_console.is_some() => {
            sprites::Pose::Type(sprites::type_frame(time.elapsed_secs()))
        }
        // In the air: rising, top, falling.
        _ if !p.grounded => sprites::Pose::Jump(sprites::jump_frame(p.vy)),
        (true, Some(f)) => sprites::Pose::Sneak(f),
        (true, None) => sprites::Pose::Crouch,
        (false, Some(f)) => sprites::Pose::Walk(f),
        (false, None) => sprites::Pose::Stand,
    };
    let half = f64::from(art.figure(sprites::Actor::Infiltrator, want).size.x * scale) * 0.5;
    // Doors stand on the floor line: nothing up on the roof or the rope.
    let on_floor = p.place == idaptik_core::scenario::state::Place::Floor;
    let shift = if on_floor {
        door_clearance(pose.x + def.player.w * 0.5, half, &sim.sim.state().doors) as f32
    } else {
        0.0
    };
    for (actor, mut root) in &mut roots {
        if *actor == sprites::Actor::Infiltrator {
            root.translation.x = shift;
        }
    }
    for (actor, pose, mut vis) in &mut poses {
        if *actor == sprites::Actor::Infiltrator {
            *vis = if *pose == want {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
    // Hidden reads as a fade, not a disappearance: you still know where you are.
    let fade = if p.hidden { 0.35 } else { 1.0 };
    for (actor, part, mut s) in &mut parts {
        if *actor == sprites::Actor::Infiltrator {
            s.color = part.0.with_alpha(part.0.alpha() * fade);
        }
    }
}

/// Billy: placed like the infiltrator, hidden while offsite, faced and scaled.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn sync_billy(
    sim: Res<SimState>,
    map: Res<SceneMap>,
    visual: Res<VisualBuffers>,
    fixed: Res<Time<Fixed>>,
    mut body: Query<(&mut Transform, &mut Sprite, &mut Visibility), With<BillyMarker>>,
    mut roots: Query<
        (&sprites::Actor, &mut Transform),
        (With<sprites::FigureRoot>, Without<BillyMarker>),
    >,
    mut poses: Query<(&sprites::Actor, &sprites::Pose, &mut Visibility), Without<BillyMarker>>,
    art: Res<sprites::PaintedSprites>,
) {
    let def = sim.sim.definition();
    // `mode` is discrete and read live; only the pose interpolates.
    let b = &sim.sim.state().billy;
    let pose = visual
        .poses
        .sample(VisualSlot::Billy.index(), render_alpha(&fixed));
    let Ok((mut tf, mut sprite, mut vis)) = body.single_mut() else {
        return;
    };
    *vis = if b.mode == BillyMode::Offsite {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    place_body(
        &map,
        &mut tf,
        &mut sprite,
        BodyBox {
            x: pose.x,
            y_top: pose.y,
            w: def.billy.w,
            h: def.billy.h,
        },
        1.0,
    );
    sprite.color = Color::NONE;
    let scale = 1.0;
    let facing = if pose.facing < 0.0 { -1.0 } else { 1.0 };
    let want = sprites::gait_frame(pose.x, b.vx, sprites::STRIDE)
        .map_or(sprites::Pose::Stand, sprites::Pose::Walk);
    for (actor, p, mut vis) in &mut poses {
        if *actor == sprites::Actor::Guard {
            *vis = if *p == want {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
    let half = f64::from(art.figure(sprites::Actor::Guard, want).size.x * scale) * 0.5;
    let shift = door_clearance(pose.x + def.billy.w * 0.5, half, &sim.sim.state().doors) as f32;
    for (actor, mut root) in &mut roots {
        if *actor == sprites::Actor::Guard {
            root.translation.x = shift;
            root.translation.y = -def.billy.h as f32 * 0.5;
            root.scale = Vec3::new(facing * scale, scale, 1.0);
        }
    }
}

/// Props: the note and USB track their state positions and vanish when
/// carried; the chute darkens open when revealed; the vacuum lights up active.
// A Bevy system takes one parameter per resource and per query, so arity here
// tracks how many things the prop layer draws, not how complex it is. Bundling
// them into a `SystemParam` would satisfy the lint and tell the reader less.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn sync_props(
    sim: Res<SimState>,
    map: Res<SceneMap>,
    visual: Res<VisualBuffers>,
    fixed: Res<Time<Fixed>>,
    mut note: Query<
        (&mut Transform, &mut Visibility),
        (With<NoteMarker>, Without<UsbMarker>, Without<VacuumMarker>),
    >,
    mut usb: Query<
        (&mut Transform, &mut Visibility, &mut Sprite),
        (With<UsbMarker>, Without<NoteMarker>, Without<VacuumMarker>),
    >,
    mut chute: Query<&mut Sprite, (With<ChuteMarker>, Without<UsbMarker>, Without<VacuumMarker>)>,
    mut vacuum: Query<
        (&mut Transform, &mut Visibility, &mut Sprite),
        (With<VacuumMarker>, Without<NoteMarker>, Without<UsbMarker>),
    >,
) {
    let s = sim.sim.state();
    let alpha = render_alpha(&fixed);
    // Positions interpolate; `held`/`billy_has`/`wiped`/`fallen` are discrete
    // and read live.

    if let Ok((mut tf, mut vis)) = note.single_mut() {
        let pose = visual.poses.sample(VisualSlot::Note.index(), alpha);
        let p = map.point(pose.x, pose.y);
        tf.translation.x = p.x;
        tf.translation.y = p.y;
        *vis = if s.note.held || s.note.billy_has {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }

    if let Ok((mut tf, mut vis, mut sprite)) = usb.single_mut() {
        let pose = visual.poses.sample(VisualSlot::Usb.index(), alpha);
        let p = map.point(pose.x, pose.y);
        tf.translation.x = p.x;
        tf.translation.y = p.y;
        *vis = if s.usb.held || s.usb.billy_has {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        // A wiped stick goes dull.
        sprite.color = if s.usb.wiped {
            Color::srgb(0.5, 0.5, 0.55)
        } else {
            Color::WHITE
        };
    }

    if let Ok(mut sprite) = chute.single_mut() {
        // Unnoticed, the chute sits back in the wall; found, it reads fully.
        sprite.color = if s.chute.revealed {
            Color::WHITE
        } else {
            Color::srgb(0.62, 0.62, 0.66)
        };
    }

    if let Ok((mut tf, mut vis, mut sprite)) = vacuum.single_mut() {
        let pose = visual.poses.sample(VisualSlot::Vacuum.index(), alpha);
        let p = map.point(pose.x, pose.y + 6.0);
        tf.translation.x = p.x;
        tf.translation.y = p.y;
        *vis = if s.vacuum.fallen {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        sprite.color = if s.vacuum.active {
            Color::WHITE
        } else {
            Color::srgb(0.7, 0.7, 0.74)
        };
    }
}

/// What the camera frames for one seat: a point (scene space) and a zoom
/// (orthographic scale; below 1 is closer).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shot {
    pub at: Vec2,
    pub scale: f32,
}

/// The kind of space a character is in. The camera frames the space, not
/// the level or the character: a room close, the outside wide, the inside of
/// the van closer still.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Space {
    Room,
    Street,
    /// On the rope, against the front facade.
    Facade,
    Roof,
    Vehicle,
}

impl Space {
    /// How close the camera sits on this space (orthographic scale; below 1
    /// is closer). Each is a whole step of the pixel grid: three screen
    /// pixels per art pixel in a room, two outside, five in the van.
    pub fn zoom(self) -> f32 {
        match self {
            Space::Room => 1.0,
            Space::Street | Space::Facade | Space::Roof => 1.5,
            Space::Vehicle => 0.6,
        }
    }
}

/// The space the infiltrator's body is in.
pub fn infiltrator_space(sim: &idaptik_core::scenario::GhostLobbySim) -> Space {
    use idaptik_core::scenario::state::Place;
    match sim.state().player.place {
        Place::Roof => Space::Roof,
        Place::Rope { .. } => Space::Facade,
        Place::Floor if sim.infiltrator_outside() => Space::Street,
        Place::Floor => Space::Room,
    }
}

/// The shot on a space, following `focus_x` (scene x) along it. Pure, so it
/// is testable.
pub fn shot_for(
    space: Space,
    focus_x: f32,
    map: &SceneMap,
    world_width: f64,
    floor: f64,
    roof_y: f64,
) -> Shot {
    let scale = space.zoom();
    let y = match space {
        Space::Vehicle => {
            let (x, y) = crate::art::van_anchor(world_width, floor);
            return Shot {
                at: map.point(x, y - 10.0),
                scale,
            };
        }
        // Each at a fixed height, so the frame never bobs.
        Space::Room => floor - 100.0,
        // The pavement and the facade above it.
        Space::Street => floor - 140.0,
        Space::Facade => (roof_y + floor) * 0.5,
        // The roof, and the sky over it.
        Space::Roof => roof_y - 60.0,
    };
    // Keep the street in frame, no further.
    let lo = map.point(-160.0, floor).x;
    let hi = map.point(world_width + 300.0, floor).x;
    Shot {
        at: Vec2::new(focus_x.clamp(lo, hi), map.point(0.0, y).y),
        scale,
    }
}

/// One frame on the outside for both characters: halfway between the
/// infiltrator and the van, from the infiltrator's level (roof or street) down
/// to the van's wheels.
pub fn merged_shot(
    space: Space,
    focus_x: f32,
    map: &SceneMap,
    world_width: f64,
    floor: f64,
    roof_y: f64,
) -> Shot {
    let (van_x, _) = crate::art::van_anchor(world_width, floor);
    let van = map.point(van_x, floor);
    let top = match space {
        Space::Roof => roof_y - 50.0,
        Space::Facade => roof_y,
        _ => floor - 140.0,
    };
    Shot {
        at: Vec2::new(
            (focus_x + van.x) * 0.5,
            map.point(0.0, (top + floor + 20.0) * 0.5).y,
        ),
        scale: Space::Street.zoom(),
    }
}

/// Each seat's camera follows its body — the infiltrator on the floor, the
/// hacker in the van — at the zoom that suits the space they are in. It eases
/// position and zoom together rather than snapping. The infiltrator's camera
/// also dives into a console's CRT when they sit at one, and pulls back out.
#[allow(clippy::too_many_arguments)]
pub fn follow_camera(
    sim: Res<SimState>,
    map: Res<SceneMap>,
    time: Res<Time>,
    mut screen: Option<ResMut<crate::screen::Screen>>,
    seats: Option<Res<crate::seats::Seats>>,
    player: Query<&Transform, (With<PlayerMarker>, Without<crate::seats::SeatCam>)>,
    mut cameras: Query<(&crate::seats::SeatCam, &mut Transform, &mut Projection)>,
    mut placed: Local<[bool; 2]>,
) {
    use crate::screen::{DIVE_SCALE, Phase, SCREEN_AT, crt_canvas};
    use idaptik_core::scenario::signal::sim::Seat;
    let def = sim.sim.definition();
    let player_x = player.single().map(|p| p.translation.x).unwrap_or(0.0);
    for (seat, mut cam, mut projection) in &mut cameras {
        let Projection::Orthographic(ortho) = &mut *projection else {
            continue;
        };
        // Net View or a screen may have switched to fit-to-size; take it back.
        if !matches!(ortho.scaling_mode, ScalingMode::WindowSize) {
            ortho.scaling_mode = ScalingMode::WindowSize;
            ortho.scale = PIXEL_SCALE;
        }
        // Shot scales are relative to the pixel-art base zoom.
        let mut zoom = ortho.scale / PIXEL_SCALE;
        let hacker = seat.0 == Seat::Hacker;
        let space = if hacker {
            Space::Vehicle
        } else {
            infiltrator_space(&sim.sim)
        };
        let roof_y = def.exterior.as_ref().map_or(CEILING_Y - 21.0, |e| e.roof_y);
        let mut shot = shot_for(
            space,
            player_x,
            &map,
            def.world.width,
            def.world.floor,
            roof_y,
        );
        // Merged: the infiltrator's camera frames both of them, outside.
        if !hacker && seats.as_ref().is_some_and(|s| s.merged) {
            shot = merged_shot(
                space,
                player_x,
                &map,
                def.world.width,
                def.world.floor,
                roof_y,
            );
        }

        // Using a console: dive into its CRT, cut to the schematic, and on
        // the way out cut back to the CRT and pull away.
        if !hacker && let Some(screen) = screen.as_deref_mut() {
            let inf = &mut screen.infiltrator;
            let crt = inf
                .console
                .as_ref()
                .and_then(|id| def.consoles.iter().find(|c| &c.id == id))
                .map(|con| {
                    let (x, y) = crt_canvas(con, def.world.width, def.world.floor);
                    map.point(x, y)
                });
            match (inf.phase, crt) {
                (Phase::DivingIn, Some(crt)) => {
                    shot = Shot {
                        at: crt,
                        scale: DIVE_SCALE,
                    };
                    if zoom <= DIVE_SCALE * 1.4 {
                        inf.phase = Phase::On;
                    }
                }
                (Phase::On, _) => {
                    cam.translation.x = SCREEN_AT.x;
                    cam.translation.y = SCREEN_AT.y + 28.0;
                    // The schematic is laid out for a fixed frame, not pixel art.
                    ortho.scaling_mode = ScalingMode::AutoMin {
                        min_width: 900.0,
                        min_height: 560.0,
                    };
                    ortho.scale = 1.0;
                    continue;
                }
                (Phase::PullingOut, Some(crt)) => {
                    if cam.translation.y < SCREEN_AT.y + 2000.0 {
                        cam.translation.x = crt.x;
                        cam.translation.y = crt.y;
                        zoom = DIVE_SCALE;
                    }
                    if (zoom - shot.scale).abs() < shot.scale * 0.05 {
                        *inf = crate::screen::SeatScreen::default();
                    }
                }
                (Phase::DivingIn | Phase::PullingOut, None) => {
                    *inf = crate::screen::SeatScreen::default();
                }
                (Phase::Closed, _) => {}
            }
        }

        let first = &mut placed[usize::from(hacker)];
        let k = if *first {
            1.0 - (-6.0 * time.delta_secs()).exp()
        } else {
            *first = true;
            1.0
        };
        cam.translation.x += (shot.at.x - cam.translation.x) * k;
        cam.translation.y += (shot.at.y - cam.translation.y) * k;
        zoom += (shot.scale - zoom) * k;
        ortho.scale = zoom * PIXEL_SCALE;
        // Snap to the screen-pixel grid so the art does not shimmer as it moves.
        let step = ortho.scale;
        cam.translation.x = (cam.translation.x / step).round() * step;
        cam.translation.y = (cam.translation.y / step).round() * step;
    }
}

/// Net View is laid out around the origin at full zoom: reset on the way in.
pub fn centre_camera(
    mut camera: Query<(&mut Transform, &mut Projection), With<crate::seats::MainCam>>,
) {
    if let Ok((mut cam, mut projection)) = camera.single_mut() {
        cam.translation.x = 0.0;
        cam.translation.y = 0.0;
        if let Projection::Orthographic(ortho) = &mut *projection {
            ortho.scaling_mode = ScalingMode::AutoMin {
                min_width: 900.0,
                min_height: 560.0,
            };
            ortho.scale = 1.0;
        }
    }
}

#[cfg(test)]
mod camera_tests {
    #[test]
    fn door_constants_match_the_door_sprite() {
        let mut images = bevy::prelude::Assets::<bevy::prelude::Image>::default();
        let art = super::sprites::PaintedSprites::new(&mut images);
        assert!((art.door.size.x - super::DOOR_W).abs() < 1e-3);
        assert!((art.door.size.y - super::DOOR_H).abs() < 1e-3);
    }

    #[test]
    fn a_figure_is_drawn_clear_of_a_closed_door_and_not_of_an_open_one() {
        use idaptik_core::scenario::state::DoorState;
        let mut door = DoorState {
            id: "d".into(),
            x: 100.0,
            open: 0.0,
            pending: 0.0,
            route_delay: 0.0,
            route_duration: 0.0,
            badge_logged: false,
            unlocked: false,
        };
        // The sim parks a centre 3 short of the plane: drawn flush, not inside.
        let dx = super::door_clearance(97.0, 8.0, std::slice::from_ref(&door));
        let flush = 100.0 - 8.0 - f64::from(super::DOOR_W) * 0.5;
        assert!((97.0 + dx - flush).abs() < 1e-9, "shift {dx}");
        // Far from any door: untouched.
        assert_eq!(
            super::door_clearance(40.0, 8.0, std::slice::from_ref(&door)),
            0.0
        );
        // An open door is walked through, not pushed off.
        door.open = 1.0;
        assert_eq!(
            super::door_clearance(97.0, 8.0, std::slice::from_ref(&door)),
            0.0
        );
    }

    use super::*;

    fn map() -> SceneMap {
        SceneMap::new(1260.0, 585.0, 0)
    }

    fn shot(space: Space, x: f32) -> Shot {
        shot_for(space, x, &map(), 1260.0, 585.0, 414.0)
    }

    #[test]
    fn the_van_is_framed_closer_than_a_room_and_the_outside_wider() {
        let room = shot(Space::Room, 0.0);
        let van = shot(Space::Vehicle, 0.0);
        let street = shot(Space::Street, 0.0);
        assert!(van.scale < room.scale);
        assert!(street.scale > room.scale);
        assert!(
            van.at.x > room.at.x,
            "the van is past the building's right wall"
        );
        // Every zoom lands on whole screen pixels per art pixel.
        for s in [
            Space::Room,
            Space::Street,
            Space::Facade,
            Space::Roof,
            Space::Vehicle,
        ] {
            let per_art = 1.0 / (s.zoom() * PIXEL_SCALE * sprites::SPRITE_PX);
            assert!((per_art - per_art.round()).abs() < 1e-3, "{s:?}: {per_art}");
        }
    }

    #[test]
    fn a_shot_tracks_x_but_holds_its_height() {
        for space in [Space::Room, Space::Street, Space::Roof] {
            let a = shot(space, -100.0);
            let b = shot(space, 200.0);
            assert_eq!(a.at.y, b.at.y);
            assert!(b.at.x > a.at.x);
        }
        // The roof is framed above the room.
        assert!(shot(Space::Roof, 0.0).at.y > shot(Space::Room, 0.0).at.y);
    }
}
