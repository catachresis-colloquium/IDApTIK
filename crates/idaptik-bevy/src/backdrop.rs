//! Room dressing: the walls behind the play, built from modular painted pieces
//! so any room, of any width or height, is dressed the same way. Render-only.
//!
//! Each room gets a [`Theme`], picked from what the room is (its id and name),
//! and from the theme:
//!
//! * **wall fill**: a texture that tiles both ways, covering the whole wall;
//! * **base band**: skirting / wainscot along the floor, tiling sideways;
//! * **cornice**: one trim along every ceiling, tiling sideways;
//! * **wall decor**: cut-outs hung on three lines (eye height, high, low);
//! * **background furniture**: cut-outs standing against the back wall.
//!
//! [`dress`] lays the decor out: each line of the wall is cut into free spans
//! around what is already there (doors, cameras, props, consoles), and the
//! theme's pieces go one by one into the middle of the widest span that still
//! fits them, until nothing more fits. A narrow room gets a clock; a wide one
//! gets the full set. The order is seeded by the room id, so the layout is
//! stable run to run and differs room to room.

use bevy::prelude::*;
use std::collections::HashMap;

use crate::scene::{CEILING_Y, GhostLobbySceneMarker, SceneMap};
use crate::sprites::{self, Painted};

// Z layers, between the room fill (0.0) and its wash (0.2).
const Z_WALL: f32 = 0.05;
const Z_BAND: f32 = 0.08;
const Z_DECOR: f32 = 0.12;
/// Background furniture: behind the hide-spot props (1.4), in front of the
/// light pools.
const Z_FURNITURE: f32 = 1.2;

/// Walls sit back: darker and cooler than anything you can touch.
const WALL_TINT: Color = Color::srgb(0.62, 0.62, 0.68);
/// Wall decor and background furniture: between the walls and the props.
const DECOR_TINT: Color = Color::srgb(0.9, 0.9, 0.93);

/// Where a wall piece hangs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Line {
    /// Pictures, boards, shelves: bottom edge above the tallest prop.
    Eye,
    /// Clocks, vents, signs, pipe runs: just under the cornice.
    High,
    /// Extinguishers: low on the wall, sharing the floor with furniture.
    Low,
    /// Furniture standing on the floor against the wall.
    Floor,
}

/// One kind of room's dressing.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    pub wall: &'static str,
    pub base: &'static str,
    /// Pieces in order of preference; a big room repeats the list.
    pub pieces: &'static [(&'static str, Line)],
}

use Line::{Eye, Floor, High, Low};

pub const THEMES: [Theme; 5] = [
    Theme {
        name: "kitchen",
        wall: "wall_kitchen",
        base: "base_kitchen",
        pieces: &[
            ("wall_cabinet", Eye),
            ("clock", High),
            ("radiator", Floor),
            ("vent", High),
            ("extinguisher", Low),
            ("water_cooler", Floor),
        ],
    },
    Theme {
        name: "hall",
        wall: "wall_hall",
        base: "base_hall",
        pieces: &[
            ("fuse_box", Eye),
            ("sconce", Eye),
            ("plant", Floor),
            ("clock", High),
            ("noticeboard", Eye),
            ("radiator", Floor),
        ],
    },
    Theme {
        name: "office",
        wall: "wall_office",
        base: "base_office",
        pieces: &[
            ("noticeboard", Eye),
            ("filing_cabinet", Floor),
            ("shelf", Eye),
            ("clock", High),
            ("plant", Floor),
            ("sconce", Eye),
        ],
    },
    Theme {
        name: "laundry",
        wall: "wall_laundry",
        base: "base_laundry",
        pieces: &[
            ("washer", Floor),
            ("pipes", High),
            ("shelf", Eye),
            ("linen_trolley", Floor),
            ("vent", High),
            ("extinguisher", Low),
        ],
    },
    Theme {
        name: "service",
        wall: "wall_exit",
        base: "base_exit",
        pieces: &[
            ("exit_sign", High),
            ("pallets", Floor),
            ("pipes", High),
            ("extinguisher", Low),
            ("vent", High),
            ("linen_trolley", Floor),
        ],
    },
];

/// Words in a room's id or name that pick its theme; anything else gets a
/// theme by hash, so a new room is always dressed.
const KEYWORDS: [(&str, usize); 14] = [
    ("kitchen", 0),
    ("break", 0),
    ("canteen", 0),
    ("hall", 1),
    ("corridor", 1),
    ("lobby", 1),
    ("office", 2),
    ("study", 2),
    ("security", 2),
    ("laundry", 3),
    ("linen", 3),
    ("utility", 3),
    ("exit", 4),
    ("loading", 4),
];

/// A cheap stable string hash (FNV-1a), for seeding layouts.
pub fn seed_of(s: &str) -> u32 {
    s.bytes().fold(0x811c_9dc5u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// The theme for a room, from its id and name.
pub fn theme_for(id: &str, name: &str) -> &'static Theme {
    let text = format!("{id} {name}").to_lowercase();
    let k = KEYWORDS
        .iter()
        .find(|(word, _)| text.contains(word))
        .map(|(_, k)| *k)
        .unwrap_or(seed_of(id) as usize % THEMES.len());
    &THEMES[k]
}

/// One piece laid out on a room's wall: its centre x and which line.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub name: &'static str,
    pub x: f64,
    pub line: Line,
}

/// What is already in a room, as `(centre, half-width)` spans per line.
#[derive(Debug, Clone, Default)]
pub struct Taken {
    pub eye: Vec<(f64, f64)>,
    pub high: Vec<(f64, f64)>,
    /// Shared by [`Line::Low`] and [`Line::Floor`].
    pub floor: Vec<(f64, f64)>,
}

/// Gap kept between pieces, and from the room's side walls (canvas units).
const GAP: f64 = 8.0;

/// Lay a theme's pieces out along a room from `x0` to `x1`. `width` gives a
/// piece's width. Pure, so it is testable for any room.
pub fn dress(
    theme: &Theme,
    x0: f64,
    x1: f64,
    taken: &Taken,
    width: impl Fn(&str) -> f64,
    seed: u32,
) -> Vec<Placed> {
    // Free spans per line: the room minus what is taken.
    let free = |spans: &[(f64, f64)]| {
        let mut out = vec![(x0 + GAP, x1 - GAP)];
        for &(c, h) in spans {
            let (a, b) = (c - h - GAP, c + h + GAP);
            out = out
                .into_iter()
                .flat_map(|(lo, hi)| {
                    if b <= lo || a >= hi {
                        vec![(lo, hi)]
                    } else {
                        [(lo, a), (b, hi)]
                            .into_iter()
                            .filter(|(l, h)| h > l)
                            .collect()
                    }
                })
                .collect();
        }
        out
    };
    let mut lines: HashMap<Line, Vec<(f64, f64)>> = HashMap::new();
    lines.insert(Eye, free(&taken.eye));
    lines.insert(High, free(&taken.high));
    let floor = free(&taken.floor);
    lines.insert(Floor, floor);

    let n = theme.pieces.len();
    let start = seed as usize % n;
    let mut placed = Vec::new();
    // Once round the list per ~400 units of wall: an ordinary room shows each
    // piece once; only a long room repeats them.
    let passes = ((x1 - x0) / 400.0).ceil().max(1.0) as usize;
    for k in 0..n * passes {
        let (name, line) = theme.pieces[(start + k) % n];
        let w = width(name);
        let key = if line == Low { Floor } else { line };
        let spans = lines.get_mut(&key).expect("every line has spans");
        let Some(i) = spans
            .iter()
            .enumerate()
            .filter(|(_, (lo, hi))| hi - lo >= w)
            .max_by(|a, b| (a.1.1 - a.1.0).total_cmp(&(b.1.1 - b.1.0)))
            .map(|(i, _)| i)
        else {
            continue;
        };
        let (lo, hi) = spans.remove(i);
        let x = (lo + hi) * 0.5;
        let (a, b) = (x - w * 0.5 - GAP, x + w * 0.5 + GAP);
        spans.extend([(lo, a), (b, hi)].into_iter().filter(|(l, h)| h > l));
        placed.push(Placed { name, x, line });
    }
    placed
}

/// Every painted backdrop piece, by name.
#[derive(Resource, Clone)]
pub struct BackdropArt(pub HashMap<&'static str, Painted>);

impl BackdropArt {
    pub fn new(images: &mut Assets<Image>) -> Self {
        macro_rules! pieces {
            ($($name:literal),* $(,)?) => {
                HashMap::from([$((
                    $name,
                    sprites::load_png(
                        images,
                        include_bytes!(concat!("../assets/backdrop/", $name, ".png")),
                    ),
                )),*])
            };
        }
        // The ceiling trim is plain enough to place exactly: a concrete band
        // with a lighter lip and a vent slot every 16 px.
        let cornice = crate::pixel::ceiling_tile(
            16,
            7,
            crate::palette::SLAB,
            crate::palette::DUST,
            crate::palette::PITCH,
        )
        .into_painted(images);
        let mut all = pieces![
            "wall_kitchen",
            "wall_hall",
            "wall_office",
            "wall_laundry",
            "wall_exit",
            "base_kitchen",
            "base_hall",
            "base_office",
            "base_laundry",
            "base_exit",
            "clock",
            "sconce",
            "fuse_box",
            "noticeboard",
            "shelf",
            "wall_cabinet",
            "vent",
            "pipes",
            "extinguisher",
            "exit_sign",
            "radiator",
            "plant",
            "filing_cabinet",
            "water_cooler",
            "washer",
            "linen_trolley",
            "pallets",
            "skyline_a",
            "skyline_b",
        ];
        all.insert("cornice", cornice);
        Self(all)
    }

    pub fn get(&self, name: &str) -> &Painted {
        self.0.get(name).expect("every theme piece is embedded")
    }
}

/// A tiled surface filling `size` (canvas units) centred at `at`.
fn tiled(p: &Painted, size: Vec2, at: Vec2, z: f32, tile_y: bool) -> impl Bundle {
    (
        GhostLobbySceneMarker,
        Sprite {
            image: p.image.clone(),
            color: WALL_TINT,
            custom_size: Some(size),
            image_mode: SpriteImageMode::Tiled {
                tile_x: true,
                tile_y,
                stretch_value: 1.0 / sprites::SPRITE_PX,
            },
            ..Default::default()
        },
        Transform::from_translation(at.extend(z)),
    )
}

/// Dress every room. Runs after the scene and the props are spawned.
pub fn setup_backdrop(
    mut commands: Commands,
    sim: Res<crate::driver::SimState>,
    mut images: ResMut<Assets<Image>>,
) {
    let def = sim.sim.definition();
    let map = SceneMap::new(def.world.width, def.world.floor, def.floor_id);
    let art = BackdropArt::new(&mut images);
    let floor = def.world.floor;
    let ceiling = CEILING_Y;
    let room_h = (floor - ceiling) as f32;
    let cornice = art.get("cornice");

    for room in &def.rooms {
        let theme = theme_for(room.id.as_str(), &room.name);
        let (x0, x1) = (room.x + 3.0, room.x + room.w - 3.0);
        let w = (x1 - x0) as f32;
        let cx = (x0 + x1) * 0.5;

        // Surfaces: fill, then the base band on the floor and the cornice
        // under the ceiling, both tiling sideways at their own height.
        let fill = art.get(theme.wall);
        commands.spawn(tiled(
            fill,
            Vec2::new(w, room_h),
            map.point(cx, (floor + ceiling) * 0.5),
            Z_WALL,
            true,
        ));
        let base = art.get(theme.base);
        commands.spawn(tiled(
            base,
            Vec2::new(w, base.size.y),
            map.point(cx, floor - f64::from(base.size.y) * 0.5),
            Z_BAND,
            false,
        ));
        commands.spawn(tiled(
            cornice,
            Vec2::new(w, cornice.size.y),
            map.point(cx, ceiling + f64::from(cornice.size.y) * 0.5),
            Z_BAND,
            false,
        ));

        // What is already on this wall.
        let inside = |x: f64| x > room.x - 20.0 && x < room.x + room.w + 20.0;
        let mut taken = Taken::default();
        for door in def.doors.iter().filter(|d| inside(d.x)) {
            taken.eye.push((door.x, 14.0));
            taken.floor.push((door.x, 18.0));
        }
        for cam in def.cameras.iter().filter(|c| inside(c.x)) {
            taken.high.push((cam.x, 16.0));
        }
        for spot in def.hide_spots.iter().filter(|s| inside(s.x)) {
            // The zone, plus room for props that stand beside it (the fridge).
            taken.floor.push((spot.x, spot.radius + 30.0));
        }
        for con in &def.consoles {
            if let idaptik_core::scenario::definition::ConsolePlace::Room { x, .. } = con.place
                && inside(x)
            {
                taken.floor.push((x, 26.0));
            }
        }

        let width = |name: &str| f64::from(art.get(name).size.x);
        let cornice_h = f64::from(cornice.size.y);
        for piece in dress(theme, x0, x1, &taken, width, seed_of(room.id.as_str())) {
            let p = art.get(piece.name);
            let h = f64::from(p.size.y);
            // Canvas y of the piece's bottom edge.
            let bottom = match piece.line {
                Eye => floor - 64.0,
                High => (ceiling + cornice_h + 4.0 + h).min(floor - 64.0),
                Low => floor - 16.0,
                Floor => floor,
            };
            let z = if piece.line == Floor {
                Z_FURNITURE
            } else {
                Z_DECOR
            };
            commands.spawn((
                GhostLobbySceneMarker,
                sprites::painted_bundle(p, map.point(piece.x, bottom), z, DECOR_TINT),
            ));
        }
    }
    commands.insert_resource(art);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widths(name: &str) -> f64 {
        match name {
            "washer" | "linen_trolley" | "pallets" | "filing_cabinet" => 30.0,
            "pipes" | "extinguisher" | "water_cooler" => 10.0,
            _ => 20.0,
        }
    }

    fn overlaps(placed: &[Placed]) -> bool {
        let same = |a: Line, b: Line| {
            let k = |l| if l == Low { Floor } else { l };
            k(a) == k(b)
        };
        placed.iter().enumerate().any(|(i, a)| {
            placed[i + 1..].iter().any(|b| {
                same(a.line, b.line) && (a.x - b.x).abs() < (widths(a.name) + widths(b.name)) * 0.5
            })
        })
    }

    #[test]
    fn every_room_gets_a_theme_by_name_or_by_hash() {
        assert_eq!(theme_for("kitchen", "Kitchen / break room").name, "kitchen");
        assert_eq!(theme_for("r7", "Security office").name, "office");
        // No keyword: still a theme, and the same one every time.
        let t = theme_for("vault-2", "Vault");
        assert_eq!(t.name, theme_for("vault-2", "Vault").name);
    }

    #[test]
    fn small_and_large_rooms_are_both_dressed_without_overlaps() {
        for (w, at_least) in [(60.0, 1), (240.0, 5), (900.0, 9)] {
            for theme in &THEMES {
                let placed = dress(theme, 0.0, w, &Taken::default(), widths, 7);
                assert!(
                    placed.len() >= at_least,
                    "{} in {w}: {placed:?}",
                    theme.name
                );
                assert!(!overlaps(&placed), "{} in {w}: {placed:?}", theme.name);
                for p in &placed {
                    let h = widths(p.name) * 0.5;
                    assert!(p.x - h >= GAP - 1e-9 && p.x + h <= w - GAP + 1e-9);
                }
            }
        }
    }

    #[test]
    fn an_ordinary_room_shows_each_piece_once() {
        for theme in &THEMES {
            let placed = dress(theme, 0.0, 300.0, &Taken::default(), widths, 11);
            let mut names: Vec<_> = placed.iter().map(|p| p.name).collect();
            let all = names.len();
            names.sort();
            names.dedup();
            assert_eq!(names.len(), all, "{}: {placed:?}", theme.name);
        }
    }

    #[test]
    fn decor_keeps_off_doors_and_hide_spots() {
        let taken = Taken {
            eye: vec![(0.0, 14.0)],
            high: vec![],
            floor: vec![(120.0, 58.0)],
        };
        let placed = dress(&THEMES[0], 0.0, 250.0, &taken, widths, 3);
        for p in &placed {
            let h = widths(p.name) * 0.5;
            match p.line {
                Eye => assert!(p.x - h > 14.0, "{p:?}"),
                Floor | Low => assert!(p.x + h < 62.0 || p.x - h > 178.0, "{p:?}"),
                High => {}
            }
        }
    }

    #[test]
    fn layout_is_stable_for_a_room_and_varies_between_rooms() {
        let t = &THEMES[1];
        let a = dress(t, 0.0, 300.0, &Taken::default(), widths, seed_of("hall"));
        assert_eq!(
            a,
            dress(t, 0.0, 300.0, &Taken::default(), widths, seed_of("hall"))
        );
        let b = dress(t, 0.0, 300.0, &Taken::default(), widths, seed_of("hall-2"));
        assert_ne!(a, b);
    }
}
