//! Raster images (via `image`) and SVG (via `resvg`) into RGBA with mipmaps.

use std::path::Path;

use image::imageops::FilterType;
use image::{DynamicImage, RgbaImage};

use crate::model::{Image, ImageLevel};

/// SVGs are rasterized so that their longer side has this many pixels.
const SVG_SIZE: u32 = 2048;

pub fn load_raster(path: &Path, max_texture: u32) -> Result<Image, String> {
    let img = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    let natural = (img.width(), img.height());
    Ok(with_mips(fit(img, max_texture).to_rgba8(), natural))
}

pub fn load_svg(path: &Path, max_texture: u32) -> Result<Image, String> {
    use resvg::{tiny_skia, usvg};

    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut options = usvg::Options {
        resources_dir: path.parent().map(Path::to_path_buf),
        ..usvg::Options::default()
    };
    // Only pay for the system font scan when the SVG has text.
    if data.windows(5).any(|w| w == b"<text") {
        options.fontdb_mut().load_system_fonts();
    }
    let tree = usvg::Tree::from_data(&data, &options).map_err(|e| e.to_string())?;
    let size = tree.size();
    let natural = (size.width().ceil() as u32, size.height().ceil() as u32);
    let longest = size.width().max(size.height()).max(1.0);
    let scale = SVG_SIZE.min(max_texture) as f32 / longest;
    let w = ((size.width() * scale).round() as u32).max(1);
    let h = ((size.height() * scale).round() as u32).max(1);
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("SVG has no size")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for p in pixmap.pixels() {
        let c = p.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let img = RgbaImage::from_raw(w, h, rgba).ok_or("bad SVG raster")?;
    Ok(with_mips(img, natural))
}

/// Downscales images larger than the GPU's texture limit.
fn fit(img: DynamicImage, max: u32) -> DynamicImage {
    if img.width() <= max && img.height() <= max {
        img
    } else {
        img.resize(max, max, FilterType::Triangle)
    }
}

/// Builds the mipmap chain (each level half the previous), for smooth downscaling.
fn with_mips(img: RgbaImage, natural: (u32, u32)) -> Image {
    let (width, height) = img.dimensions();
    let mut levels = Vec::new();
    let mut current = img;
    loop {
        let (w, h) = current.dimensions();
        let next = (w > 1 || h > 1).then(|| {
            image::imageops::resize(
                &current,
                (w / 2).max(1),
                (h / 2).max(1),
                FilterType::Triangle,
            )
        });
        levels.push(ImageLevel {
            width: w,
            height: h,
            rgba: current.into_raw(),
        });
        match next {
            Some(n) => current = n,
            None => break,
        }
    }
    Image {
        width,
        height,
        natural,
        levels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mip_chain_halves_down_to_one_pixel() {
        let img = with_mips(RgbaImage::new(8, 3), (8, 3));
        let sizes: Vec<_> = img.levels.iter().map(|l| (l.width, l.height)).collect();
        assert_eq!(sizes, [(8, 3), (4, 1), (2, 1), (1, 1)]);
        assert!(
            img.levels
                .iter()
                .all(|l| l.rgba.len() == (l.width * l.height * 4) as usize)
        );
    }

    #[test]
    fn rasterizes_svg() {
        let dir = std::env::temp_dir().join(format!("mao-docs-svg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.svg");
        std::fs::write(
            &file,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="red"/></svg>"#,
        )
        .unwrap();
        let img = load_svg(&file, 4096).unwrap();
        assert_eq!((img.width, img.height), (2048, 1024));
        assert_eq!(img.natural, (20, 10));
        assert_eq!(&img.levels[0].rgba[..4], &[255, 0, 0, 255]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
