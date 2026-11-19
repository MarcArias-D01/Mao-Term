/// Converts an sRGB-encoded channel in `0.0..=1.0` to linear light.
pub fn srgb_to_linear(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Converts an sRGB color to what the render target expects: linear values for
/// sRGB targets (the hardware re-encodes them), sRGB values otherwise.
pub(crate) fn to_target([r, g, b]: [u8; 3], srgb_target: bool) -> [f32; 4] {
    let convert = |c: u8| {
        let v = f64::from(c) / 255.0;
        (if srgb_target { srgb_to_linear(v) } else { v }) as f32
    };
    [convert(r), convert(g), convert(b), 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_preserved() {
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn mid_grey() {
        // sRGB 0.5 ≈ 0.214 linear.
        assert!((srgb_to_linear(0.5) - 0.214).abs() < 1e-3);
    }

    #[test]
    fn target_conversion() {
        assert_eq!(to_target([255, 0, 255], false), [1.0, 0.0, 1.0, 1.0]);
        assert!((to_target([128, 128, 128], true)[0] - 0.2158).abs() < 1e-3);
    }
}
