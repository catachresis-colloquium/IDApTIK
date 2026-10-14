//! The UI's pixel font. One of three bundled OFL pixel fonts (see
//! `assets/fonts/`) replaces Bevy's default font, so every HUD line, log and
//! screen label uses it without each text site naming a font; and every text
//! is drawn without anti-aliasing at a size on the font's own pixel grid, so
//! glyphs stay crisp.
//!
//! `IDAPTIK_UI_FONT=silkscreen|vt323|pixelify` picks the font (default
//! [`UiFont::DEFAULT`]).

use bevy::prelude::*;
use bevy::text::{Font, FontSize, FontSmoothing};

/// The bundled pixel fonts.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiFont {
    /// Silkscreen: blocky capitals, 8 px grid.
    Silkscreen,
    /// VT323: a terminal face, 16 px grid.
    Vt323,
    /// Pixelify Sans: a readable pixel sans, 8 px grid.
    Pixelify,
}

impl UiFont {
    pub const DEFAULT: UiFont = UiFont::Vt323;

    /// From `IDAPTIK_UI_FONT`, else the default.
    pub fn from_env() -> Self {
        match std::env::var("IDAPTIK_UI_FONT").as_deref() {
            Ok("silkscreen") => UiFont::Silkscreen,
            Ok("vt323") => UiFont::Vt323,
            Ok("pixelify") => UiFont::Pixelify,
            _ => UiFont::DEFAULT,
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            UiFont::Silkscreen => include_bytes!("../assets/fonts/Silkscreen-Regular.ttf"),
            UiFont::Vt323 => include_bytes!("../assets/fonts/VT323-Regular.ttf"),
            UiFont::Pixelify => include_bytes!("../assets/fonts/PixelifySans.ttf"),
        }
    }

    /// The size, in pixels, the font's glyphs are drawn on: sizes are kept to
    /// whole multiples of it.
    pub fn grid(self) -> f32 {
        match self {
            UiFont::Silkscreen | UiFont::Pixelify => 8.0,
            UiFont::Vt323 => 16.0,
        }
    }

    /// A requested size snapped to the font's grid (never below one grid).
    pub fn snap(self, size: f32) -> f32 {
        let g = self.grid();
        ((size / g).round() * g).max(g)
    }
}

/// Put the chosen font into Bevy's default font slot.
pub fn install_ui_font(mut commands: Commands, fonts: Option<ResMut<Assets<Font>>>) {
    let choice = UiFont::from_env();
    commands.insert_resource(choice);
    let Some(mut fonts) = fonts else {
        return;
    };
    let font = Font::from_bytes(choice.bytes().to_vec());
    // The default handle is what every `TextFont::default()` points at.
    let _ = fonts.insert(&Handle::<Font>::default(), font);
}

/// Every new text: no anti-aliasing, size on the font's grid.
pub fn crisp_text(font: Option<Res<UiFont>>, mut texts: Query<&mut TextFont, Added<TextFont>>) {
    let choice = font.map_or(UiFont::DEFAULT, |f| *f);
    for mut text in &mut texts {
        text.font_smoothing = FontSmoothing::None;
        if let FontSize::Px(px) = text.font_size {
            text.font_size = FontSize::Px(choice.snap(px));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_snap_to_the_font_grid() {
        assert_eq!(UiFont::Silkscreen.snap(13.0), 16.0);
        assert_eq!(UiFont::Silkscreen.snap(3.0), 8.0);
        assert_eq!(UiFont::Vt323.snap(22.0), 16.0);
        assert_eq!(UiFont::Vt323.snap(26.0), 32.0);
    }

    #[test]
    fn every_bundled_font_parses() {
        for f in [UiFont::Silkscreen, UiFont::Vt323, UiFont::Pixelify] {
            let _ = Font::from_bytes(f.bytes().to_vec());
        }
    }
}
