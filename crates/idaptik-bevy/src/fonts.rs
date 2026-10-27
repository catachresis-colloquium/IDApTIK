//! The UI's pixel font. One of three bundled OFL pixel fonts (see
//! `assets/fonts/`) replaces Bevy's default font, so every HUD line, log and
//! screen label uses it without each text site naming a font; and every text
//! is drawn without anti-aliasing at a size on the font's own pixel grid, so
//! glyphs stay crisp.
//!
//! `IDAPTIK_UI_FONT=silkscreen|vt323|pixelify` picks the font (default
//! [`UiFont::DEFAULT`]).
//!
//! The faces are the brand's `font` tokens (`config/brand-tokens.ncl`): each
//! face's grid comes from `idaptik_brand::generated::font`, and the tests
//! below hold the rest to the tokens -- the default face, the names
//! `IDAPTIK_UI_FONT` accepts, and the bytes embedded for each token's file.
//! `include_bytes!` needs a literal path, so the files cannot be read from the
//! constants directly; the test that compares the embedded bytes with the
//! token's file on disk is what keeps them the same file.

use bevy::prelude::*;
use bevy::text::{Font, FontSize, FontSmoothing};
use idaptik_brand::generated::font as tok;

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
    /// The brand's default face (`font::DEFAULT`; pinned by a test).
    pub const DEFAULT: UiFont = UiFont::Vt323;

    /// Every bundled face.
    pub const ALL: [UiFont; 3] = [UiFont::Silkscreen, UiFont::Vt323, UiFont::Pixelify];

    /// From `IDAPTIK_UI_FONT`, else the default.
    pub fn from_env() -> Self {
        std::env::var("IDAPTIK_UI_FONT")
            .ok()
            .and_then(|name| Self::from_token_name(&name))
            .unwrap_or(UiFont::DEFAULT)
    }

    /// The face whose brand token is called `name` (`vt323`, `silkscreen`,
    /// `pixelify`).
    pub fn from_token_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.token_name() == name)
    }

    /// This face's brand token name, which is also its `IDAPTIK_UI_FONT` value.
    pub fn token_name(self) -> &'static str {
        match self {
            UiFont::Silkscreen => "silkscreen",
            UiFont::Vt323 => "vt323",
            UiFont::Pixelify => "pixelify",
        }
    }

    /// The family name, from the brand token.
    pub fn family(self) -> &'static str {
        match self {
            UiFont::Silkscreen => tok::SILKSCREEN,
            UiFont::Vt323 => tok::VT323,
            UiFont::Pixelify => tok::PIXELIFY,
        }
    }

    /// The file under `assets/fonts/`, from the brand token.
    pub fn file(self) -> &'static str {
        match self {
            UiFont::Silkscreen => tok::SILKSCREEN_FILE,
            UiFont::Vt323 => tok::VT323_FILE,
            UiFont::Pixelify => tok::PIXELIFY_FILE,
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
    /// whole multiples of it. Read from the brand token, so a grid edited in
    /// `config/brand-tokens.ncl` and regenerated changes what the game draws.
    pub fn grid(self) -> f32 {
        let px = match self {
            UiFont::Silkscreen => tok::SILKSCREEN_GRID_PX,
            UiFont::Vt323 => tok::VT323_GRID_PX,
            UiFont::Pixelify => tok::PIXELIFY_GRID_PX,
        };
        px as f32
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
        for f in UiFont::ALL {
            let _ = Font::from_bytes(f.bytes().to_vec());
        }
    }

    #[test]
    fn the_default_face_is_the_brand_default() {
        assert_eq!(UiFont::DEFAULT.family(), tok::DEFAULT);
    }

    #[test]
    fn the_faces_are_exactly_the_font_tokens() {
        // A bijection in both directions: every token names a face here, and
        // every face here is a token, with the same family, file and grid.
        assert_eq!(UiFont::ALL.len(), tok::ALL.len());
        for &(name, family, file, grid_px, default) in tok::ALL {
            let f = UiFont::from_token_name(name)
                .unwrap_or_else(|| panic!("font token {name} has no UiFont"));
            assert_eq!(f.family(), family, "{name}");
            assert_eq!(f.file(), file, "{name}");
            assert_eq!(f.grid(), grid_px as f32, "{name}");
            assert_eq!(f == UiFont::DEFAULT, default, "{name}");
        }
        for f in UiFont::ALL {
            assert_eq!(UiFont::from_token_name(f.token_name()), Some(f));
        }
    }

    #[test]
    fn the_embedded_bytes_are_the_token_file() {
        // `include_bytes!` takes a literal path, so this is what proves the
        // literal and the token name the same file.
        for f in UiFont::ALL {
            let path = format!("{}/assets/fonts/{}", env!("CARGO_MANIFEST_DIR"), f.file());
            let on_disk = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
            assert!(
                f.bytes() == on_disk.as_slice(),
                "{:?} embeds a different file from {path}",
                f
            );
        }
    }
}
