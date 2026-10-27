//! The brand sheet: every colour token and the type ramp in the chosen face,
//! drawn by the real frontend, so what it shows is what the game draws.
//!
//! ```text
//! cargo run -p idaptik-bevy --example brand_sheet [-- --screenshot PATH [--frames N]]
//! IDAPTIK_UI_FONT=silkscreen|vt323|pixelify picks the face (see fonts.rs).
//! ```
//!
//! With `--screenshot` the sheet saves a PNG of the window after `--frames`
//! frames and exits, so it can run headless:
//!
//! ```text
//! WGPU_BACKEND=gl xvfb-run -a cargo run -p idaptik-bevy --example brand_sheet -- \
//!     --screenshot docs/design/brand/brand-sheet-vt323.png
//! ```
//!
//! Every value on the sheet comes from `idaptik_brand::generated`, the
//! constants rendered from `config/brand-tokens.ncl`; nothing is typed in
//! here, so the sheet cannot disagree with the tokens.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::window::WindowResolution;
use clap::Parser;
use idaptik_bevy::fonts::{self, UiFont};
use idaptik_bevy::palette;
use idaptik_brand::generated::{colour, font, size};
use std::path::PathBuf;

/// Render the IDApTIK brand sheet.
#[derive(Parser, Debug, Clone, Resource)]
#[command(name = "brand_sheet")]
struct Cli {
    /// Save a PNG of the window after `--frames` frames, then exit.
    #[arg(long, value_name = "PATH")]
    screenshot: Option<PathBuf>,

    /// Frames to draw before the screenshot (fonts and layout need a few).
    #[arg(long, default_value_t = 30)]
    frames: u32,
}

fn main() -> AppExit {
    let cli = Cli::parse();
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "IDApTIK — brand sheet".to_owned(),
            resolution: WindowResolution::new(1120, 820),
            ..Default::default()
        }),
        ..Default::default()
    }))
    .insert_resource(ClearColor(palette::PITCH))
    .insert_resource(cli)
    .add_systems(
        Startup,
        (
            fonts::install_ui_font,
            setup_sheet.after(fonts::install_ui_font),
        ),
    )
    .add_systems(Update, (fonts::crisp_text, shoot));
    app.run()
}

/// The brand token the chosen face corresponds to: `(family, grid)`.
fn face_of(font: UiFont) -> (&'static str, u32) {
    match font {
        UiFont::Vt323 => (font::VT323, font::VT323_GRID_PX),
        UiFont::Silkscreen => (font::SILKSCREEN, font::SILKSCREEN_GRID_PX),
        UiFont::Pixelify => (font::PIXELIFY, font::PIXELIFY_GRID_PX),
    }
}

fn text(s: impl Into<String>, px: u32, color: Color) -> (Text, TextFont, TextColor) {
    (
        Text::new(s),
        TextFont::from_font_size(px as f32),
        TextColor(color),
    )
}

fn setup_sheet(mut commands: Commands, font: Res<UiFont>) {
    commands.spawn(Camera2d);
    let (family, grid) = face_of(*font);

    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(16.0)),
            row_gap: Val::Px(10.0),
            ..Default::default()
        })
        .with_children(|root| {
            root.spawn(text(
                format!("IDApTIK brand sheet: {family} on a {grid} px grid"),
                size::PX16,
                palette::BONE,
            ));

            // Swatches, one row per group, each labelled with name and hex.
            for group in ["ground", "accent", "scene"] {
                root.spawn(text(group, size::PX11, palette::DUST));
                root.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(8.0),
                    row_gap: Val::Px(6.0),
                    ..Default::default()
                })
                .with_children(|row| {
                    for &(name, g, seat, rgb) in colour::ALL.iter().filter(|t| t.1 == group) {
                        debug_assert_eq!(g, group);
                        row.spawn(Node {
                            width: Val::Px(92.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(2.0),
                            ..Default::default()
                        })
                        .with_children(|cell| {
                            cell.spawn((
                                Node {
                                    width: Val::Px(92.0),
                                    height: Val::Px(40.0),
                                    border: UiRect::all(Val::Px(1.0)),
                                    ..Default::default()
                                },
                                BackgroundColor(palette::hex(rgb)),
                                BorderColor::all(palette::SLAB),
                            ));
                            let label = if seat == "none" {
                                name.to_owned()
                            } else {
                                format!("{name} ({seat})")
                            };
                            cell.spawn(text(label, size::PX10, palette::BONE));
                            cell.spawn(text(format!("#{rgb:06x}"), size::PX10, palette::DUST));
                        });
                    }
                });
            }

            // The type ramp: each requested size, and what the face snaps it to.
            root.spawn(text(
                "type ramp: requested px -> drawn px (snapped to the face's grid)",
                size::PX11,
                palette::DUST,
            ));
            for &(name, px) in size::ALL {
                let drawn = font.snap(px as f32);
                root.spawn(text(
                    format!(
                        "{name} -> {drawn} px   The hacker holds the doors; the infiltrator holds the floor."
                    ),
                    px,
                    palette::BONE,
                ));
            }
        });
}

/// With `--screenshot`, capture the window after `--frames` frames and exit
/// once the capture lands; fail loudly if it never does.
fn shoot(
    mut commands: Commands,
    cli: Res<Cli>,
    mut frame: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = cli.screenshot.as_ref() else {
        return;
    };
    *frame += 1;
    if *frame == cli.frames {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path.clone()))
            .observe(
                |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                    exit.write(AppExit::Success);
                },
            );
    }
    if *frame > cli.frames + 300 {
        eprintln!("brand_sheet: the screenshot was never captured");
        exit.write(AppExit::error());
    }
}
