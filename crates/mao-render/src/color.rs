/// Converts an sRGB-encoded channel in `0.0..=1.0` to linear light.
pub fn srgb_to_linear(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
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
}
