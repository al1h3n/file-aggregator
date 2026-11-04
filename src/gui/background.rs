//! What sits behind the glass: a solid colour, a gradient or a photo.
//!
//! The choice is saved between launches (see `App::save`). Colours are stored as 8-bit
//! sRGB, the way people pick them; the GPU gets linear RGB (solid) or OKLab (gradient).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 8-bit sRGB colour.
pub type Rgb = [u8; 3];

/// Neutral grey the app opens with.
pub const DEFAULT_GREY: Rgb = [0x8E, 0x8E, 0x93];

/// Colour swatches in the background picker: name, colour.
pub const SOLIDS: &[(&str, Rgb)] = &[
    ("Grey", DEFAULT_GREY),
    ("Graphite", [0x3A, 0x3A, 0x3C]),
    ("Sand", [0xD9, 0xCF, 0xBF]),
    ("Sage", [0x8F, 0xA3, 0x8D]),
    ("Mist", [0x9C, 0xB1, 0xC4]),
    ("Clay", [0xB9, 0x8B, 0x76]),
];

/// Gradient swatches: name, top-left colour, bottom-right colour.
pub const GRADIENTS: &[(&str, Rgb, Rgb)] = &[
    ("Dusk", [0x2E, 0x2A, 0x5C], [0xE3, 0x8B, 0x6B]),
    ("Lagoon", [0x0E, 0x4D, 0x6E], [0x7D, 0xD3, 0xC0]),
    ("Meadow", [0x2F, 0x5D, 0x50], [0xC9, 0xD8, 0x8E]),
    ("Blossom", [0xF6, 0xD5, 0xC8], [0x9A, 0x8F, 0xD8]),
    ("Night", [0x10, 0x14, 0x2A], [0x4A, 0x3F, 0x7A]),
];

/// The window background.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Background {
    /// One colour.
    Solid(Rgb),
    /// Diagonal gradient from the top-left to the bottom-right corner.
    Gradient(Rgb, Rgb),
    /// An image file, scaled to cover the window.
    Photo(PathBuf),
}

impl Default for Background {
    fn default() -> Self {
        Self::Solid(DEFAULT_GREY)
    }
}

impl Background {
    /// Perceived lightness `0..=1` (OKLab L). `None` for a photo: that is measured once it loads.
    pub fn lightness(&self) -> Option<f32> {
        match self {
            Self::Solid(c) => Some(oklab(*c)[0]),
            Self::Gradient(a, b) => Some((oklab(*a)[0] + oklab(*b)[0]) * 0.5),
            Self::Photo(_) => None,
        }
    }
}

/// Above this lightness the light colour scheme is easier to read (dark text on light glass).
pub const LIGHT_THRESHOLD: f32 = 0.6;

/// sRGB 8-bit to linear `0..=1`.
pub fn linear(c: Rgb) -> [f32; 3] {
    c.map(|v| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    })
}

/// sRGB 8-bit to OKLab (L, a, b), Björn Ottosson's matrices.
pub fn oklab(c: Rgb) -> [f32; 3] {
    let [r, g, b] = linear(c);
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// A decoded photo, ready for the GPU.
pub struct Photo {
    pub image: image::RgbaImage,
    /// Average perceived lightness, to pick the colour scheme.
    pub lightness: f32,
}

/// Longest side kept on the GPU. Bigger photos are downscaled once on load: the glass
/// frosts most of the window anyway, and it keeps a 50-megapixel photo from taking
/// 200 MB of video memory.
const MAX_PHOTO_SIDE: u32 = 3840;

/// Decodes `path` (honouring the EXIF orientation of phone photos) and downscales it.
/// Slow for big images (~100–300 ms): call it from a background thread.
pub fn load_photo(path: &Path) -> Result<Photo> {
    use image::ImageDecoder;
    let mut decoder = image::ImageReader::open(path)
        .with_context(|| format!("Can't open {}", path.display()))?
        .with_guessed_format()?
        .into_decoder()
        .context("Unsupported image format")?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = image::DynamicImage::from_decoder(decoder).context("Can't decode image")?;
    image.apply_orientation(orientation);
    if image.width().max(image.height()) > MAX_PHOTO_SIDE {
        // `thumbnail` is a fast box filter: right for big downscales, no aliasing.
        image = image.thumbnail(MAX_PHOTO_SIDE, MAX_PHOTO_SIDE);
    }
    let image = image.to_rgba8();
    let lightness = average_lightness(&image);
    Ok(Photo { image, lightness })
}

/// Mean luminance of a 32×32 thumbnail, as OKLab-style lightness (cube root of Y).
fn average_lightness(image: &image::RgbaImage) -> f32 {
    let small = image::imageops::thumbnail(image, 32, 32);
    let n = (small.width() * small.height()).max(1) as f32;
    let y: f32 = small
        .pixels()
        .map(|p| {
            let [r, g, b] = linear([p[0], p[1], p[2]]);
            0.2126 * r + 0.7152 * g + 0.0722 * b
        })
        .sum::<f32>()
        / n;
    y.cbrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oklab_reference_values() {
        // White is L = 1, black L = 0, greys have no chroma.
        let white = oklab([255, 255, 255]);
        assert!((white[0] - 1.0).abs() < 1e-3 && white[1].abs() < 1e-3 && white[2].abs() < 1e-3);
        assert!(oklab([0, 0, 0])[0].abs() < 1e-6);
        let grey = oklab(DEFAULT_GREY);
        assert!(grey[1].abs() < 0.01 && grey[2].abs() < 0.01);
    }

    #[test]
    fn default_background_is_grey_and_light() {
        assert_eq!(Background::default(), Background::Solid(DEFAULT_GREY));
        assert!(Background::default().lightness().unwrap() > LIGHT_THRESHOLD);
        assert!(Background::Solid([0x3A, 0x3A, 0x3C]).lightness().unwrap() < LIGHT_THRESHOLD);
    }

    #[test]
    fn photo_is_downscaled_and_measured() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wide.png");
        image::RgbaImage::from_pixel(4000, 1000, image::Rgba([250, 250, 250, 255])).save(&path).unwrap();
        let photo = load_photo(&path).unwrap();
        assert_eq!(photo.image.dimensions(), (3840, 960));
        assert!(photo.lightness > 0.95);
        assert!(load_photo(&dir.path().join("missing.png")).is_err());
    }
}
