//! Startup-only PDF palettes. A worker and its texture caches share one copy
//! for their entire lifetime; `night` selects the palette in render keys.
use eframe::egui::Color32;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Rgb(pub [u8; 3]);

impl TryFrom<String> for Rgb {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let bytes = value.as_bytes();
        if bytes.len() != 7 || bytes[0] != b'#' || !bytes[1..].iter().all(u8::is_ascii_hexdigit) {
            return Err(format!("Invalid color {value:?}: expected #RRGGBB"));
        }
        Ok(Self(std::array::from_fn(|channel| {
            u8::from_str_radix(&value[1 + channel * 2..3 + channel * 2], 16).unwrap()
        })))
    }
}

impl From<Rgb> for String {
    fn from(Rgb([r, g, b]): Rgb) -> Self {
        format!("#{r:02x}{g:02x}{b:02x}")
    }
}

impl Rgb {
    pub fn color(self) -> Color32 {
        Color32::from_rgb(self.0[0], self.0[1], self.0[2])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PagePalette {
    /// Output for black source pixels.
    pub foreground: Rgb,
    /// Output for white source pixels.
    pub background: Rgb,
    /// Area outside the paper; omitted values follow the interface theme.
    pub canvas: Option<Rgb>,
}

impl Default for PagePalette {
    fn default() -> Self {
        Self {
            foreground: Rgb([0; 3]),
            background: Rgb([255; 3]),
            canvas: None,
        }
    }
}

impl PagePalette {
    pub fn inverted() -> Self {
        Self {
            foreground: Rgb([255; 3]),
            background: Rgb([0; 3]),
            canvas: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PageColors {
    pub white: PagePalette,
    pub inverted: InvertedPalette,
}

// Use the same per-field defaults when an inverted palette is only partially
// configured (serde's struct defaults cannot depend on its parent field).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InvertedPalette {
    pub foreground: Rgb,
    pub background: Rgb,
    pub canvas: Option<Rgb>,
}

impl Default for InvertedPalette {
    fn default() -> Self {
        let palette = PagePalette::inverted();
        Self {
            foreground: palette.foreground,
            background: palette.background,
            canvas: None,
        }
    }
}

impl PageColors {
    pub fn palette(self, night: bool) -> PagePalette {
        if night {
            PagePalette {
                foreground: self.inverted.foreground,
                background: self.inverted.background,
                canvas: self.inverted.canvas,
            }
        } else {
            self.white
        }
    }
}
