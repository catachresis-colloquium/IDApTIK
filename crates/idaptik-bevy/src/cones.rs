//! Vision cones as pixel art: the cameras' sweeps and Billy's sight, drawn
//! each frame into one transparent overlay at the sprites' art resolution, so
//! they step on the same pixel grid as everything else instead of being thin
//! smooth lines. A cone is a translucent fill with a one-pixel edge.
//!
//! The cones use the exact sweep the simulation's camera system uses (a pure
//! view: nothing here can disagree with detection).

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use idaptik_core::scenario::camera_node_id;
use idaptik_core::scenario::common::{BillyMode, Phase};
use idaptik_core::scenario::constants as c;

use crate::driver::SimState;
use crate::palette;
use crate::scene::{CEILING_Y, GhostLobbySceneMarker, SceneMap};
use crate::sprites::SPRITE_PX;

/// Over the props and doors, under the people.
const Z_CONES: f32 = 3.5;

/// Fill and edge alpha of a live cone, and of a dead one (looped, blinded,
/// switched off).
const LIVE: (f32, f32) = (0.16, 0.5);
const DEAD: (f32, f32) = (0.05, 0.14);

/// The overlay image and the canvas rectangle it covers.
#[derive(Resource)]
pub struct ConeOverlay {
    image: Handle<Image>,
    /// Canvas x and y of the overlay's top-left corner.
    origin: (f64, f64),
    w: usize,
    h: usize,
}

/// A triangle in overlay pixels.
type Tri = [(f32, f32); 3];

/// Rasterise `tri` into an RGBA8 buffer (`w` x `h`): pixels whose centre is
/// inside get `fill`, and inside pixels with a 4-neighbour outside get `edge`
/// (the stepped one-pixel outline). Overlaps keep the stronger alpha. Pure, so
/// it is testable.
pub fn fill_cone(buf: &mut [u8], w: usize, h: usize, tri: Tri, rgb: [u8; 3], fill: f32, edge: f32) {
    let inside = |x: i64, y: i64| {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            return false;
        }
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let side =
            |a: (f32, f32), b: (f32, f32)| (b.0 - a.0) * (py - a.1) - (b.1 - a.1) * (px - a.0);
        let (d0, d1, d2) = (
            side(tri[0], tri[1]),
            side(tri[1], tri[2]),
            side(tri[2], tri[0]),
        );
        (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
    };
    let xs = tri.map(|p| p.0);
    let ys = tri.map(|p| p.1);
    let lo = |v: [f32; 3]| v.iter().cloned().fold(f32::MAX, f32::min).floor() as i64;
    let hi = |v: [f32; 3]| v.iter().cloned().fold(f32::MIN, f32::max).ceil() as i64;
    for y in lo(ys).max(0)..=hi(ys).min(h as i64 - 1) {
        for x in lo(xs).max(0)..=hi(xs).min(w as i64 - 1) {
            if !inside(x, y) {
                continue;
            }
            let rim =
                !inside(x - 1, y) || !inside(x + 1, y) || !inside(x, y - 1) || !inside(x, y + 1);
            let a = ((if rim { edge } else { fill }) * 255.0) as u8;
            let i = (y as usize * w + x as usize) * 4;
            if a > buf[i + 3] {
                buf[i..i + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], a]);
            }
        }
    }
}

fn rgb(color: Color) -> [u8; 3] {
    let s = color.to_srgba();
    [
        (s.red * 255.0) as u8,
        (s.green * 255.0) as u8,
        (s.blue * 255.0) as u8,
    ]
}

/// Spawn the overlay over the building's rooms. Runs after the scene setup.
pub fn setup_cones(mut commands: Commands, sim: Res<SimState>, mut images: ResMut<Assets<Image>>) {
    let def = sim.sim.definition();
    let map = SceneMap::new(def.world.width, def.world.floor, def.floor_id);
    let (wu, hu) = (def.world.width, def.world.floor - CEILING_Y);
    let w = (wu * f64::from(SPRITE_PX)).ceil() as usize;
    let h = (hu * f64::from(SPRITE_PX)).ceil() as usize;
    let mut image = Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0; w * h * 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    let handle = images.add(image);
    let size = Vec2::new(w as f32, h as f32) / SPRITE_PX;
    commands.spawn((
        GhostLobbySceneMarker,
        Sprite {
            image: handle.clone(),
            custom_size: Some(size),
            ..Default::default()
        },
        Transform::from_translation(
            map.box_center(0.0, CEILING_Y, f64::from(size.x), f64::from(size.y))
                .extend(Z_CONES),
        ),
    ));
    commands.insert_resource(ConeOverlay {
        image: handle,
        origin: (0.0, CEILING_Y),
        w,
        h,
    });
}

/// Redraw every cone for this frame.
pub fn draw_cones(
    sim: Res<SimState>,
    overlay: Option<Res<ConeOverlay>>,
    painted: Option<Res<crate::sprites::PaintedSprites>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(overlay), Some(painted)) = (overlay, painted) else {
        return;
    };
    let camera_w_px = f64::from(painted.camera.size.x * SPRITE_PX);
    let guard_h = f64::from(painted.guard_stand.size.y);
    let Some(mut image) = images.get_mut(&overlay.image) else {
        return;
    };
    let Some(buf) = image.data.as_mut() else {
        return;
    };
    buf.fill(0);
    let (w, h) = (overlay.w, overlay.h);
    let px = |x: f64, y: f64| {
        (
            ((x - overlay.origin.0) * f64::from(SPRITE_PX)) as f32,
            ((y - overlay.origin.1) * f64::from(SPRITE_PX)) as f32,
        )
    };
    let def = sim.sim.definition();
    let s = sim.sim.state();
    let floor = def.world.floor;

    // Cameras: the sim's own sweep.
    let all_off = s.camera_lockout > 0.0 || s.lights_flicker > 0.0;
    for (i, cam) in def.cameras.iter().enumerate() {
        let sweep = (s.t * c::CAM_SWEEP_W + cam.phase).sin() * c::CAM_SWEEP_A;
        let lo = cam.range.0 + sweep.min(0.0);
        let hi = cam.range.1 + sweep.max(0.0);
        let looped = s.camera_looped.get(i).is_some_and(|left| *left > 0.0);
        let dead = s.dead_nodes.contains(&camera_node_id(i));
        let blind = sim.sim.signal_blinds_camera(i);
        let live = !(cam.stale || looped || dead || all_off || blind);
        let (color, (fill, edge)) = if live {
            (palette::SIGNAL, LIVE)
        } else {
            (palette::DUST, DEAD)
        };
        // From the sprite's lens, mirrored with the sprite (`art::camera_lens`).
        let lens = crate::art::camera_lens(cam, camera_w_px);
        let tri = [px(lens.0, lens.1), px(lo, floor), px(hi, floor)];
        fill_cone(buf, w, h, tri, rgb(color), fill, edge);
    }

    // Billy's sight, from head height, once he is on the floor in a crisis.
    let b = &s.billy;
    if b.mode != BillyMode::Offsite && s.phase == Phase::Crisis {
        let reach = def
            .difficulty
            .get(&sim.sim.config().difficulty)
            .map(|p| p.billy_sight)
            .unwrap_or(200.0);
        let facing = if b.facing < 0.0 { -1.0 } else { 1.0 };
        let eye = (
            b.x + def.billy.w * 0.5,
            billy_eye_y(b.y, def.billy.h, guard_h),
        );
        let tri = [
            px(eye.0, eye.1),
            px(eye.0 + facing * reach, floor - 70.0),
            px(eye.0 + facing * reach, floor),
        ];
        fill_cone(buf, w, h, tri, rgb(palette::ALARM), LIVE.0, LIVE.1);
    }
}

/// Art pixels from the top of the guard's sprite down to his eyes.
const GUARD_EYE_PX: f64 = 6.0;

/// Canvas y of Billy's eyes: his sprite stands on the floor at his box's
/// feet, and is shorter than the box, so the eyes are measured down from the
/// sprite's top, not the box's.
pub fn billy_eye_y(box_top: f64, box_h: f64, sprite_h: f64) -> f64 {
    box_top + box_h - sprite_h + GUARD_EYE_PX / f64::from(SPRITE_PX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn billys_sight_starts_at_his_drawn_eyes() {
        let mut images = Assets::<Image>::default();
        let art = crate::sprites::PaintedSprites::new(&mut images);
        let def = idaptik_core::scenario::ghost_lobby();
        let sprite_h = f64::from(art.guard_stand.size.y);
        let top = 400.0;
        let eye = billy_eye_y(top, def.billy.h, sprite_h);
        let head = top + def.billy.h - sprite_h;
        // Below the top of his head, and in its upper part.
        assert!(eye > head, "the cone starts above his head");
        assert!(eye < head + 10.0 / f64::from(SPRITE_PX));
    }

    #[test]
    fn a_cone_is_a_filled_triangle_with_a_one_pixel_edge() {
        let (w, h) = (40, 30);
        let mut buf = vec![0u8; w * h * 4];
        fill_cone(
            &mut buf,
            w,
            h,
            [(20.0, 0.0), (0.0, 30.0), (40.0, 30.0)],
            [255, 0, 0],
            0.2,
            0.6,
        );
        let a = |x: usize, y: usize| buf[(y * w + x) * 4 + 3];
        let (fill, edge) = ((0.2f32 * 255.0) as u8, (0.6f32 * 255.0) as u8);
        assert_eq!(a(20, 20), fill, "deep inside is fill");
        assert_eq!(a(20, 29), edge, "the bottom row is edge");
        assert_eq!(a(1, 2), 0, "outside stays clear");
        // Every inside pixel is either fill or edge, nothing in between.
        assert!((0..w * h).all(|i| [0, fill, edge].contains(&buf[i * 4 + 3])));
    }

    #[test]
    fn overlapping_cones_keep_the_stronger_alpha() {
        let (w, h) = (10, 10);
        let mut buf = vec![0u8; w * h * 4];
        let tri = [(5.0, 0.0), (0.0, 10.0), (10.0, 10.0)];
        fill_cone(&mut buf, w, h, tri, [255, 0, 0], 0.5, 0.5);
        fill_cone(&mut buf, w, h, tri, [0, 255, 0], 0.1, 0.1);
        assert_eq!(
            &buf[(8 * w + 5) * 4..(8 * w + 5) * 4 + 4],
            &[255, 0, 0, 127]
        );
    }
}
