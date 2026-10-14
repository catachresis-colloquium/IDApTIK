//! Sprites: the pixel-art figures and props on the floor, and the layered
//! device and network icons Net View draws.
//!
//! The figures and props are pixel art at one fixed density (`assets/sprites/`,
//! embedded at build time so nothing is loaded from disk at run time), drawn
//! at their true size and sampled nearest-neighbour so pixels stay square. A
//! hidden infiltrator fades by tinting sprite alpha alone.

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;

//## Painted sprites

/// Art pixels per canvas unit: every sprite is pixel art at this density (the
/// 54-unit infiltrator is 48 px tall), so its drawn size follows from its pixel
/// size alone, and nothing is scaled.
pub const SPRITE_PX: f32 = 48.0 / 54.0;

/// One painted sprite and its size in canvas units.
#[derive(Clone)]
pub struct Painted {
    pub image: Handle<Image>,
    pub size: Vec2,
}

/// Every painted sprite, decoded once at start-up.
#[derive(Resource, Clone)]
pub struct PaintedSprites {
    pub infiltrator_stand: Painted,
    pub infiltrator_crouch: Painted,
    pub guard_stand: Painted,
    pub hacker_sit: Painted,
    pub van: Painted,
    pub counter: Painted,
    pub fridge: Painted,
    pub copier: Painted,
    pub desk: Painted,
    pub locker: Painted,
    pub crate_: Painted,
    pub crt_console: Painted,
    pub door: Painted,
    pub camera: Painted,
    pub ceiling_light: Painted,
    pub note: Painted,
    pub usb: Painted,
    pub chute: Painted,
    pub vacuum: Painted,
    pub badge_reader: Painted,
    pub exit_door: Painted,
    /// Walk cycles, [`WALK_FRAMES`] each, near foot forward first.
    pub infiltrator_walk: Vec<Painted>,
    pub infiltrator_sneak: Vec<Painted>,
    /// Rising, top, falling.
    pub infiltrator_jump: Vec<Painted>,
    pub guard_walk: Vec<Painted>,
    pub hacker_walk: Vec<Painted>,
    /// Seated typing loops, [`TYPE_FRAMES`] each.
    pub infiltrator_type: Vec<Painted>,
    pub hacker_type: Vec<Painted>,
}

/// Decode one embedded PNG cut at [`SPRITE_PX`]; its size follows from its pixels.
pub fn load_png(images: &mut Assets<Image>, png: &[u8]) -> Painted {
    let image = Image::from_buffer(
        png,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::nearest(),
        RenderAssetUsages::RENDER_WORLD,
    )
    .expect("embedded sprite is a valid PNG");
    let size = Vec2::new(image.width() as f32, image.height() as f32) / SPRITE_PX;
    Painted {
        image: images.add(image),
        size,
    }
}

impl PaintedSprites {
    pub fn new(images: &mut Assets<Image>) -> Self {
        let mut load = |png: &[u8]| load_png(images, png);
        macro_rules! png {
            ($name:literal) => {
                load(include_bytes!(concat!("../assets/sprites/", $name, ".png")))
            };
        }
        macro_rules! frames {
            ($name:literal, $($n:literal),+) => {
                vec![$(load(include_bytes!(concat!("../assets/sprites/", $name, "_", stringify!($n), ".png")))),+]
            };
        }
        Self {
            infiltrator_stand: png!("infiltrator_stand"),
            infiltrator_crouch: png!("infiltrator_crouch"),
            guard_stand: png!("guard_stand"),
            hacker_sit: png!("hacker_sit"),
            van: png!("van"),
            counter: png!("counter"),
            fridge: png!("fridge"),
            copier: png!("copier"),
            desk: png!("desk"),
            locker: png!("locker"),
            crate_: png!("crate"),
            crt_console: png!("crt_console"),
            door: png!("door"),
            camera: png!("camera"),
            ceiling_light: png!("ceiling_light"),
            note: png!("note"),
            usb: png!("usb"),
            chute: png!("chute"),
            vacuum: png!("vacuum"),
            badge_reader: png!("badge_reader"),
            exit_door: png!("exit_door"),
            infiltrator_walk: frames!("infiltrator_walk", 0, 1, 2, 3, 4, 5),
            infiltrator_sneak: frames!("infiltrator_sneak", 0, 1, 2, 3, 4, 5),
            infiltrator_jump: frames!("infiltrator_jump", 0, 1, 2),
            guard_walk: frames!("guard_walk", 0, 1, 2, 3, 4, 5),
            hacker_walk: frames!("hacker_walk", 0, 1, 2, 3, 4, 5),
            infiltrator_type: frames!("infiltrator_type", 0, 1, 2, 3),
            hacker_type: frames!("hacker_type", 0, 1, 2, 3),
        }
    }

    /// The figure for an actor in a pose. Every painted figure faces right.
    pub fn figure(&self, actor: Actor, pose: Pose) -> &Painted {
        match (actor, pose) {
            (Actor::Infiltrator, Pose::Walk(f)) => cycle(&self.infiltrator_walk, f),
            (Actor::Infiltrator, Pose::Sneak(f)) => cycle(&self.infiltrator_sneak, f),
            (Actor::Infiltrator, Pose::Type(f)) => cycle(&self.infiltrator_type, f),
            (Actor::Infiltrator, Pose::Jump(f)) => cycle(&self.infiltrator_jump, f),
            (Actor::Guard, Pose::Walk(f)) => cycle(&self.guard_walk, f),
            (Actor::Hacker, Pose::Walk(f)) => cycle(&self.hacker_walk, f),
            (Actor::Hacker, Pose::Type(f)) => cycle(&self.hacker_type, f),
            (Actor::Infiltrator, Pose::Crouch) => &self.infiltrator_crouch,
            (Actor::Infiltrator, _) => &self.infiltrator_stand,
            (Actor::Guard, _) => &self.guard_stand,
            (Actor::Hacker, _) => &self.hacker_sit,
        }
    }
}

/// A painted sprite standing with its bottom centre at `at`, tinted `color`
/// (white leaves it as painted).
pub fn painted_bundle(p: &Painted, at: Vec2, z: f32, color: Color) -> impl Bundle {
    (
        Sprite {
            image: p.image.clone(),
            color,
            custom_size: Some(p.size),
            ..Default::default()
        },
        Transform::from_translation((at + Vec2::new(0.0, p.size.y * 0.5)).extend(z)),
    )
}

//## Person

/// Whose figure a part belongs to, so each sync system touches only its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub enum Actor {
    Infiltrator,
    Guard,
    Hacker,
}

/// A figure's pose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub enum Pose {
    Stand,
    Sit,
    Crouch,
    /// A walk-cycle frame.
    Walk(u8),
    /// A crouched sneak-cycle frame.
    Sneak(u8),
    /// A seated typing frame.
    Type(u8),
    /// In the air: 0 rising, 1 at the top, 2 falling.
    Jump(u8),
}

/// Frames in a walk or sneak cycle, and in a typing loop.
pub const WALK_FRAMES: u8 = 6;
pub const TYPE_FRAMES: u8 = 4;
/// Typing speed (steps a second).
pub const TYPE_FPS: f32 = 12.0;
/// The typing frames in order: 0 and 2 are the hands at rest, 1 the front
/// hand lifted, 3 the back hand. An irregular run of taps with the odd pause
/// reads as typing; a strict alternation reads as a machine.
const TYPE_STEPS: [u8; 24] = [
    1, 0, 3, 0, 1, 3, 0, 1, 0, 0, 3, 1, 0, 3, 0, 1, 1, 0, 3, 0, 2, 2, 2, 0,
];

fn cycle(frames: &[Painted], f: u8) -> &Painted {
    &frames[usize::from(f) % frames.len()]
}

impl Pose {
    /// Every pose a figure has, for spawning one child per pose.
    pub fn all_for(actor: Actor) -> Vec<Pose> {
        let walk = |f: fn(u8) -> Pose| (0..WALK_FRAMES).map(f).collect::<Vec<_>>();
        let typing = || (0..TYPE_FRAMES).map(Pose::Type).collect::<Vec<_>>();
        match actor {
            Actor::Infiltrator => [
                vec![Pose::Stand, Pose::Crouch],
                walk(Pose::Walk),
                walk(Pose::Sneak),
                typing(),
                (0..3).map(Pose::Jump).collect(),
            ]
            .concat(),
            Actor::Guard => [vec![Pose::Stand], walk(Pose::Walk)].concat(),
            Actor::Hacker => [vec![Pose::Sit], typing(), walk(Pose::Walk)].concat(),
        }
    }
}

/// Canvas units walked per walk-cycle frame: a cycle (two steps of about
/// 45 units, roughly as long as the drawn legs reach) spans the six frames.
/// At the player's walking speed that is about 11 frames a second.
pub const STRIDE: f64 = 15.0;
/// The same for the sneak: shorter, bent-kneed steps (about 8 frames a second
/// at crouching speed).
pub const SNEAK_STRIDE: f64 = 10.0;
/// Below this speed (canvas units a second) a figure stands rather than walks.
pub const WALK_MIN_VX: f64 = 18.0;

/// The jump frame for a vertical speed (canvas units a second, down positive):
/// rising, near the top, falling.
pub fn jump_frame(vy: f64) -> u8 {
    if vy < -120.0 {
        0
    } else if vy < 120.0 {
        1
    } else {
        2
    }
}

/// The typing-loop frame at time `t` (seconds).
pub fn type_frame(t: f32) -> u8 {
    TYPE_STEPS[(t.max(0.0) * TYPE_FPS) as usize % TYPE_STEPS.len()]
}

/// The walk-cycle frame for a figure at `x` moving at `vx` with `stride`
/// units per frame, or none when it is standing still. Chosen by distance
/// walked, not time, so feet never slide.
pub fn gait_frame(x: f64, vx: f64, stride: f64) -> Option<u8> {
    (vx.abs() >= WALK_MIN_VX).then(|| ((x.abs() / stride) as u64 % u64::from(WALK_FRAMES)) as u8)
}

/// Marker: the root a figure's parts hang from (feet at its origin).
#[derive(Component)]
pub struct FigureRoot;

/// One part of a figure, remembering its own colour so a fade can be applied
/// as alpha without losing it.
#[derive(Component, Clone, Copy)]
pub struct FigurePart(pub Color);

/// The white disc image the light pools and wheels tint.
#[derive(Resource, Clone)]
pub struct FigureImages {
    pub disc: Handle<Image>,
}

impl FigureImages {
    /// Rasterise the disc once (64 px, anti-aliased edge).
    pub fn new(images: &mut Assets<Image>) -> Self {
        Self {
            disc: images.add(disc_image()),
        }
    }
}

fn disc_image() -> Image {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    const N: u32 = 64;
    let mut data = Vec::with_capacity((N * N * 4) as usize);
    let c = (N as f32 - 1.0) * 0.5;
    for y in 0..N {
        for x in 0..N {
            let (dx, dy) = (x as f32 - c, y as f32 - c);
            let a = (c + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            data.extend_from_slice(&[255, 255, 255, (a * 255.0) as u8]);
        }
    }
    Image::new(
        Extent3d {
            width: N,
            height: N,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// Spawn a full figure with one child per pose in `poses`, all hidden but the
/// first. Coordinates are canvas units with the feet at the origin; the caller
/// finds the poses by [`Pose`].
pub fn spawn_person(
    parent: &mut ChildSpawnerCommands,
    art: &PaintedSprites,
    actor: Actor,
    poses: &[Pose],
) {
    parent
        .spawn((
            actor,
            FigureRoot,
            Transform::default(),
            Visibility::Inherited,
        ))
        .with_children(|root| {
            for (k, pose) in poses.iter().enumerate() {
                root.spawn((
                    actor,
                    *pose,
                    FigurePart(Color::WHITE),
                    painted_bundle(art.figure(actor, *pose), Vec2::ZERO, 0.1, Color::WHITE),
                    if k == 0 {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    },
                ));
            }
        });
}

#[cfg(test)]
mod gait_tests {
    use super::*;

    #[test]
    fn standing_still_has_no_frame_and_walking_steps_through_every_frame() {
        assert_eq!(gait_frame(100.0, 0.0, STRIDE), None);
        assert_eq!(gait_frame(100.0, WALK_MIN_VX * 0.5, STRIDE), None);
        let frames: Vec<u8> = (0..6)
            .map(|k| gait_frame(k as f64 * STRIDE + 0.5, 60.0, STRIDE).unwrap())
            .collect();
        assert_eq!(frames, [0, 1, 2, 3, 4, 5]);
        // Walking left steps the same cycle.
        assert!(gait_frame(-50.0, -60.0, STRIDE).is_some());
    }

    #[test]
    fn the_walk_plays_at_a_readable_rate() {
        // Pixel walk cycles read at roughly 8-14 frames a second.
        let fps = |speed: f64, stride: f64| speed / stride;
        let walk = fps(
            idaptik_core::scenario::constants::STORY_PLAYER_SPEED,
            STRIDE,
        );
        let sneak = fps(
            idaptik_core::scenario::constants::STORY_PLAYER_SPEED
                * idaptik_core::scenario::constants::CROUCH_SPEED,
            SNEAK_STRIDE,
        );
        assert!((8.0..=14.0).contains(&walk), "walk {walk} fps");
        assert!((6.0..=12.0).contains(&sneak), "sneak {sneak} fps");
    }

    #[test]
    fn typing_steps_only_through_the_typing_frames() {
        assert!(TYPE_STEPS.iter().all(|&f| f < TYPE_FRAMES));
        let frames: std::collections::HashSet<u8> =
            (0..48).map(|k| type_frame(k as f32 / TYPE_FPS)).collect();
        assert_eq!(frames.len(), usize::from(TYPE_FRAMES));
    }

    #[test]
    fn a_jump_rises_peaks_and_falls() {
        assert_eq!(jump_frame(-400.0), 0);
        assert_eq!(jump_frame(0.0), 1);
        assert_eq!(jump_frame(400.0), 2);
    }
}

//## Devices

use idaptik_core::device::DeviceKind;

/// The seven original-taxonomy device sprites, ported from `WorldScreen.res`'s
/// `WorldDeviceIcon`, plus three originally-designed fallbacks (`SmartDoor`,
/// `Light`, `Substation`) for kinds that appear on the grounded network graph
/// but have no source art, and a plain box for every other kind (the UMS
/// parity/physical-function kinds this game added after the port), so the
/// caller never has to special-case an unknown one.
pub fn spawn_device_icon(
    commands: &mut ChildSpawnerCommands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    kind: DeviceKind,
) {
    match kind {
        DeviceKind::Laptop | DeviceKind::Desktop => {
            // Desk plus a screen-and-base laptop, transliterated from
            // WorldScreen.res's `Laptop` arm.
            commands.spawn((
                Sprite::from_color(Color::srgb(0.29, 0.22, 0.16), Vec2::new(70.0, 20.0)),
                Transform::from_translation(Vec3::new(0.0, -10.0, 0.0)),
            ));
            commands.spawn((
                Sprite::from_color(Color::srgb(0.2, 0.2, 0.2), Vec2::new(50.0, 35.0)),
                Transform::from_translation(Vec3::new(0.0, 17.5, 0.1)),
            ));
            commands.spawn((
                Sprite::from_color(Color::srgb(0.0, 0.6, 0.9), Vec2::new(44.0, 29.0)),
                Transform::from_translation(Vec3::new(0.0, 17.5, 0.2)),
            ));
        }
        DeviceKind::Server => {
            // A rack of six units, each with a green LED.
            commands.spawn((
                Sprite::from_color(Color::srgb(0.2, 0.2, 0.2), Vec2::new(50.0, 120.0)),
                Transform::from_translation(Vec3::new(0.0, 60.0, 0.0)),
            ));
            for i in 0..6 {
                let y = 10.0 + i as f32 * 18.0;
                commands.spawn((
                    Sprite::from_color(Color::srgb(0.0, 0.6, 0.9), Vec2::new(44.0, 16.0)),
                    Transform::from_translation(Vec3::new(0.0, y, 0.1)),
                ));
                let led_mesh = meshes.add(Circle::new(3.0));
                let led_material = materials.add(ColorMaterial::from(Color::srgb(0.0, 1.0, 0.0)));
                commands.spawn((
                    Mesh2d(led_mesh),
                    MeshMaterial2d(led_material),
                    Transform::from_translation(Vec3::new(17.0, y + 8.0, 0.2)),
                ));
            }
        }
        DeviceKind::Router | DeviceKind::AccessPoint | DeviceKind::Switch => {
            // Shelf, box, two antennas, four LEDs.
            commands.spawn((
                Sprite::from_color(Color::srgb(0.29, 0.22, 0.16), Vec2::new(60.0, 8.0)),
                Transform::from_translation(Vec3::new(0.0, -36.0, 0.0)),
            ));
            commands.spawn((
                Sprite::from_color(Color::srgb(0.0, 0.6, 0.9), Vec2::new(50.0, 20.0)),
                Transform::from_translation(Vec3::new(0.0, -20.0, 0.1)),
            ));
            for x in [-16.0, 16.0] {
                commands.spawn((
                    Sprite::from_color(Color::srgb(0.2, 0.2, 0.2), Vec2::new(4.0, 25.0)),
                    Transform::from_translation(Vec3::new(x, 2.5, 0.1)),
                ));
            }
            for i in 0..4 {
                let x = -15.0 + i as f32 * 10.0;
                let led_mesh = meshes.add(Circle::new(3.0));
                let led_material = materials.add(ColorMaterial::from(Color::srgb(0.0, 1.0, 0.0)));
                commands.spawn((
                    Mesh2d(led_mesh),
                    MeshMaterial2d(led_material),
                    Transform::from_translation(Vec3::new(x, -10.0, 0.2)),
                ));
            }
        }
        DeviceKind::IotCamera | DeviceKind::Camera => {
            // Pole, mount, lensed body -- reused for both the network-graph
            // IotCamera and the physical-function Camera actuator, since both
            // read visually as "a camera".
            commands.spawn((
                Sprite::from_color(Color::srgb(0.4, 0.4, 0.4), Vec2::new(8.0, 100.0)),
                Transform::from_translation(Vec3::new(0.0, -50.0, 0.0)),
            ));
            commands.spawn((
                Sprite::from_color(Color::srgb(0.27, 0.27, 0.27), Vec2::new(30.0, 10.0)),
                Transform::from_translation(Vec3::new(0.0, -5.0, 0.1)),
            ));
            let body_mesh = meshes.add(Circle::new(18.0));
            let body_material = materials.add(ColorMaterial::from(Color::srgb(0.0, 0.6, 0.9)));
            commands.spawn((
                Mesh2d(body_mesh),
                MeshMaterial2d(body_material),
                Transform::from_translation(Vec3::new(0.0, 20.0, 0.2)),
            ));
            let lens_mesh = meshes.add(Circle::new(10.0));
            let lens_material = materials.add(ColorMaterial::from(Color::srgb(0.13, 0.13, 0.13)));
            commands.spawn((
                Mesh2d(lens_mesh),
                MeshMaterial2d(lens_material),
                Transform::from_translation(Vec3::new(0.0, 20.0, 0.3)),
            ));
        }
        DeviceKind::Terminal => {
            // Desk, CRT monitor, terminal-green text, keyboard.
            commands.spawn((
                Sprite::from_color(Color::srgb(0.29, 0.22, 0.16), Vec2::new(80.0, 30.0)),
                Transform::from_translation(Vec3::new(0.0, -15.0, 0.0)),
            ));
            commands.spawn((
                Sprite::from_color(Color::srgb(0.2, 0.2, 0.2), Vec2::new(60.0, 50.0)),
                Transform::from_translation(Vec3::new(0.0, 25.0, 0.1)),
            ));
            commands.spawn((
                Sprite::from_color(Color::BLACK, Vec2::new(54.0, 44.0)),
                Transform::from_translation(Vec3::new(0.0, 25.0, 0.2)),
            ));
            commands.spawn((
                Text2d::new(">_"),
                TextFont::from_font_size(16.0),
                TextColor(Color::srgb(0.0, 1.0, 0.0)),
                Transform::from_translation(Vec3::new(0.0, 25.0, 0.3)),
            ));
            commands.spawn((
                Sprite::from_color(Color::srgb(0.13, 0.13, 0.13), Vec2::new(50.0, 8.0)),
                Transform::from_translation(Vec3::new(0.0, -2.0, 0.1)),
            ));
        }
        DeviceKind::PowerStation | DeviceKind::Substation => {
            // Industrial cabinet, ventilation grille, power meter, warning
            // stripe, status light. Substation has no source art of its own;
            // it is conceptually a power station, so it reuses this look.
            commands.spawn((
                Sprite::from_color(Color::srgb(0.16, 0.16, 0.16), Vec2::new(80.0, 150.0)),
                Transform::from_translation(Vec3::new(0.0, 75.0, 0.0)),
            ));
            for i in 0..6 {
                let y = 10.0 + i as f32 * 12.0;
                commands.spawn((
                    Sprite::from_color(Color::srgb(0.1, 0.1, 0.1), Vec2::new(60.0, 8.0)),
                    Transform::from_translation(Vec3::new(0.0, y, 0.1)),
                ));
            }
            commands.spawn((
                Sprite::from_color(Color::srgb(0.95, 0.8, 0.0), Vec2::new(80.0, 20.0)),
                Transform::from_translation(Vec3::new(0.0, -10.0, 0.1)),
            ));
            let light_mesh = meshes.add(Circle::new(8.0));
            let light_material = materials.add(ColorMaterial::from(Color::srgb(0.0, 1.0, 0.0)));
            commands.spawn((
                Mesh2d(light_mesh),
                MeshMaterial2d(light_material),
                Transform::from_translation(Vec3::new(0.0, 140.0, 0.2)),
            ));
        }
        DeviceKind::Ups => {
            // Smaller box with an LCD display and status LEDs.
            commands.spawn((
                Sprite::from_color(Color::srgb(0.24, 0.24, 0.24), Vec2::new(60.0, 80.0)),
                Transform::from_translation(Vec3::new(0.0, 40.0, 0.0)),
            ));
            commands.spawn((
                Sprite::from_color(Color::srgb(0.1, 0.23, 0.1), Vec2::new(40.0, 20.0)),
                Transform::from_translation(Vec3::new(0.0, 65.0, 0.1)),
            ));
            for x in [-10.0, 0.0, 10.0] {
                let led_mesh = meshes.add(Circle::new(4.0));
                let led_material = materials.add(ColorMaterial::from(Color::srgb(0.0, 1.0, 0.0)));
                commands.spawn((
                    Mesh2d(led_mesh),
                    MeshMaterial2d(led_material),
                    Transform::from_translation(Vec3::new(x, 40.0, 0.1)),
                ));
            }
        }
        DeviceKind::SmartDoor => {
            // Reuses the existing scene door-slab look (a plain slab); Net
            // View draws it with a neutral colour since there is no live
            // door-state to key off here, unlike `scene::sync_doors`.
            commands.spawn((
                Sprite::from_color(Color::srgb(0.5, 0.5, 0.55), Vec2::new(12.0, 96.0)),
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
            ));
        }
        DeviceKind::Light => {
            // A small fixture: a circle with a downward cone.
            let mesh = meshes.add(Circle::new(6.0));
            let material = materials.add(ColorMaterial::from(Color::srgb(0.95, 0.9, 0.6)));
            commands.spawn((
                Mesh2d(mesh),
                MeshMaterial2d(material),
                Transform::from_translation(Vec3::new(0.0, 10.0, 0.0)),
            ));
            commands.spawn((
                Sprite::from_color(Color::srgba(0.95, 0.9, 0.6, 0.3), Vec2::new(20.0, 14.0)),
                Transform::from_translation(Vec3::new(0.0, 0.0, -0.1)),
            ));
        }
        // Everything else (Firewall, Lock, Elevator, Sensor, and the UMS
        // parity kinds PatchPanel/FibreHub/PhoneSystem/PowerSupply) has no
        // source art and no bespoke design of its own yet: a plain neutral
        // box, so the caller never has to match on kind itself.
        _ => {
            commands.spawn((
                Sprite::from_color(Color::srgb(0.4, 0.4, 0.42), Vec2::new(40.0, 40.0)),
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
            ));
        }
    }
}

//## Net View icons

use idaptik_core::device::SecurityLevel;

/// The flat coloured-square-with-emblem style Net View's node grid uses,
/// ported from IDApixiTIK's `NetworkDesktop.res` -- the actual "Network View
/// (Debug)" screen behind `WorldScreen.res`'s debug button. This is a
/// different, much simpler style than `spawn_device_icon`'s physical-world
/// props above, which belong to placed world objects, not this abstract
/// graph view.
const NET_ICON_SIZE: f32 = 52.0;
const NET_ICON_BORDER: f32 = 4.0;

/// The square's fill colour, ported from `DeviceTypes.res`'s `getDeviceColor`
/// (the seven original-taxonomy kinds) plus `Firewall`'s own dark red (the
/// source detects "firewall" by an IP heuristic on a plain Router; this
/// taxonomy already has a distinct `Firewall` kind, so it gets the colour
/// directly) and originally-designed neutral fills for every kind with no
/// source art.
fn net_icon_colour(kind: DeviceKind) -> Color {
    match kind {
        DeviceKind::Laptop | DeviceKind::Desktop => Color::srgb_u8(0x21, 0x96, 0xf3),
        DeviceKind::Router | DeviceKind::AccessPoint | DeviceKind::Switch => {
            Color::srgb_u8(0xff, 0x98, 0x00)
        }
        DeviceKind::Firewall => Color::srgb_u8(0xd3, 0x2f, 0x2f),
        DeviceKind::Server => Color::srgb_u8(0x9c, 0x27, 0xb0),
        DeviceKind::IotCamera | DeviceKind::Camera => Color::srgb_u8(0xf4, 0x43, 0x36),
        DeviceKind::Terminal => Color::srgb_u8(0x4c, 0xaf, 0x50),
        DeviceKind::PowerStation | DeviceKind::Substation => Color::srgb_u8(0xff, 0xeb, 0x3b),
        DeviceKind::Ups => Color::srgb_u8(0x79, 0x55, 0x48),
        DeviceKind::SmartDoor => Color::srgb_u8(0x75, 0x75, 0x75),
        DeviceKind::Light => Color::srgb_u8(0xff, 0xf5, 0x9d),
        _ => Color::srgb_u8(0x61, 0x61, 0x61),
    }
}

/// The security-level dot colour, ported verbatim from `getSecurityColor`.
fn net_icon_security_colour(level: SecurityLevel) -> Color {
    match level {
        SecurityLevel::Open => Color::srgb(0.0, 1.0, 0.0),
        SecurityLevel::Weak => Color::srgb(1.0, 1.0, 0.0),
        SecurityLevel::Medium => Color::srgb(1.0, 0.596, 0.0),
        SecurityLevel::Strong => Color::srgb(1.0, 0.0, 0.0),
    }
}

/// The white-silhouette emblem drawn over the coloured square, ported from
/// `createDeviceGraphic` (the seven original kinds) plus original, simple
/// glyphs for `Firewall`, `SmartDoor` and `Light`; every other kind gets none.
fn spawn_net_icon_emblem(
    parent: &mut ChildSpawnerCommands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    kind: DeviceKind,
) {
    let white = Color::WHITE;
    match kind {
        DeviceKind::Laptop | DeviceKind::Desktop => {
            parent.spawn((
                Sprite::from_color(white, Vec2::new(20.0, 12.0)),
                Transform::from_translation(Vec3::new(0.0, 3.0, 0.1)),
            ));
            parent.spawn((
                Sprite::from_color(white, Vec2::new(10.0, 1.5)),
                Transform::from_translation(Vec3::new(0.0, -5.0, 0.1)),
            ));
        }
        DeviceKind::Router | DeviceKind::AccessPoint | DeviceKind::Switch => {
            let mesh = meshes.add(Circle::new(7.0));
            let material = materials.add(ColorMaterial::from(white));
            parent.spawn((
                Mesh2d(mesh),
                MeshMaterial2d(material),
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.1)),
            ));
            parent.spawn((
                Sprite::from_color(white, Vec2::new(2.0, 8.0)),
                Transform::from_translation(Vec3::new(0.0, 8.0, 0.1)),
            ));
        }
        DeviceKind::Firewall => {
            for (x, y, w) in [
                (0.0, 8.0, 15.0),
                (-4.0, 2.0, 6.0),
                (4.5, 2.0, 8.0),
                (0.0, -4.0, 15.0),
            ] {
                parent.spawn((
                    Sprite::from_color(white, Vec2::new(w, 4.0)),
                    Transform::from_translation(Vec3::new(x, y, 0.1)),
                ));
            }
        }
        DeviceKind::Server => {
            for y in [8.0, 0.0, -8.0] {
                parent.spawn((
                    Sprite::from_color(white, Vec2::new(20.0, 4.0)),
                    Transform::from_translation(Vec3::new(0.0, y, 0.1)),
                ));
            }
        }
        DeviceKind::IotCamera | DeviceKind::Camera => {
            let mesh = meshes.add(Circle::new(6.0));
            let material = materials.add(ColorMaterial::from(white));
            parent.spawn((
                Mesh2d(mesh),
                MeshMaterial2d(material),
                Transform::from_translation(Vec3::new(0.0, 3.0, 0.1)),
            ));
            parent.spawn((
                Sprite::from_color(white, Vec2::new(5.0, 4.0)),
                Transform::from_translation(Vec3::new(0.0, -4.0, 0.1)),
            ));
        }
        DeviceKind::Terminal => {
            parent.spawn((
                Sprite::from_color(Color::BLACK, Vec2::new(25.0, 15.0)),
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.1)),
            ));
            parent.spawn((
                Text2d::new(">_"),
                TextFont::from_font_size(10.0),
                TextColor(Color::srgb(0.0, 1.0, 0.0)),
                Transform::from_translation(Vec3::new(-6.0, 0.0, 0.2)),
            ));
        }
        DeviceKind::PowerStation | DeviceKind::Substation => {
            parent.spawn((
                Sprite::from_color(white, Vec2::new(15.0, 17.5)),
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.1)),
            ));
            for y in [4.0, -1.0, -6.0] {
                parent.spawn((
                    Sprite::from_color(Color::srgb_u8(0xff, 0xeb, 0x3b), Vec2::new(10.0, 2.5)),
                    Transform::from_translation(Vec3::new(0.0, y, 0.2)),
                ));
            }
        }
        DeviceKind::Ups => {
            parent.spawn((
                Sprite::from_color(white, Vec2::new(17.5, 12.5)),
                Transform::from_translation(Vec3::new(-1.0, 0.0, 0.1)),
            ));
            for x in [-4.0, 4.0] {
                parent.spawn((
                    Sprite::from_color(Color::srgb(0.0, 1.0, 0.0), Vec2::new(5.0, 7.5)),
                    Transform::from_translation(Vec3::new(x, 0.0, 0.2)),
                ));
            }
        }
        DeviceKind::SmartDoor => {
            parent.spawn((
                Sprite::from_color(white, Vec2::new(10.0, 22.0)),
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.1)),
            ));
            let mesh = meshes.add(Circle::new(1.5));
            let material = materials.add(ColorMaterial::from(Color::BLACK));
            parent.spawn((
                Mesh2d(mesh),
                MeshMaterial2d(material),
                Transform::from_translation(Vec3::new(3.0, 0.0, 0.2)),
            ));
        }
        DeviceKind::Light => {
            let mesh = meshes.add(Circle::new(6.0));
            let material = materials.add(ColorMaterial::from(white));
            parent.spawn((
                Mesh2d(mesh),
                MeshMaterial2d(material),
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.1)),
            ));
        }
        _ => {}
    }
}

/// Spawn a Net View node icon: a bordered coloured square, its emblem, and a
/// security-level dot. Callers add the name label separately, matching the
/// existing pattern in `net_view.rs`'s `setup_net_view`.
pub fn spawn_network_icon(
    parent: &mut ChildSpawnerCommands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    kind: DeviceKind,
    security: SecurityLevel,
) {
    let full = NET_ICON_SIZE + NET_ICON_BORDER;
    parent.spawn((
        Sprite::from_color(Color::BLACK, Vec2::splat(full)),
        Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
    ));
    parent
        .spawn((
            Sprite::from_color(net_icon_colour(kind), Vec2::splat(NET_ICON_SIZE)),
            Transform::from_translation(Vec3::new(0.0, 0.0, 0.05)),
            Visibility::Inherited,
        ))
        .with_children(|square| {
            spawn_net_icon_emblem(square, meshes, materials, kind);
        });
    let dot_mesh = meshes.add(Circle::new(4.0));
    let dot_material = materials.add(ColorMaterial::from(net_icon_security_colour(security)));
    parent.spawn((
        Mesh2d(dot_mesh),
        MeshMaterial2d(dot_material),
        Transform::from_translation(Vec3::new(
            NET_ICON_SIZE * 0.5 - 4.0,
            NET_ICON_SIZE * 0.5 - 4.0,
            0.2,
        )),
    ));
}

#[cfg(test)]
mod device_icon_tests {
    use super::*;
    use idaptik_core::device::DeviceKind;

    /// Every `DeviceKind` must spawn something -- this is the whole contract
    /// Net View (Part C) leans on: it never has to match on kind itself.
    #[test]
    fn every_device_kind_spawns_without_panicking() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<Assets<Mesh>>();
        app.init_resource::<Assets<ColorMaterial>>();
        let kinds = [
            DeviceKind::Laptop,
            DeviceKind::Router,
            DeviceKind::Server,
            DeviceKind::IotCamera,
            DeviceKind::Terminal,
            DeviceKind::PowerStation,
            DeviceKind::Ups,
            DeviceKind::Firewall,
            DeviceKind::SmartDoor,
            DeviceKind::Camera,
            DeviceKind::Lock,
            DeviceKind::Elevator,
            DeviceKind::Light,
            DeviceKind::Sensor,
            DeviceKind::Substation,
            DeviceKind::PatchPanel,
            DeviceKind::FibreHub,
            DeviceKind::PhoneSystem,
            DeviceKind::AccessPoint,
            DeviceKind::Switch,
            DeviceKind::Desktop,
            DeviceKind::PowerSupply,
        ];
        for kind in kinds {
            app.world_mut()
                .resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
                    world.resource_scope(|world, mut materials: Mut<Assets<ColorMaterial>>| {
                        let mut commands = world.commands();
                        let root = commands.spawn_empty().id();
                        commands.entity(root).with_children(|parent| {
                            spawn_device_icon(parent, &mut meshes, &mut materials, kind);
                        });
                    });
                });
        }
        app.update();
    }
}
