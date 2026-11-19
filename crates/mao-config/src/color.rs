use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer};

/// An sRGB color, written in config files as `"#rrggbb"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const fn to_array(self) -> [u8; 3] {
        [self.r, self.g, self.b]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid color {0:?}, expected \"#rrggbb\"")]
pub struct ParseRgbError(String);

impl FromStr for Rgb {
    type Err = ParseRgbError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseRgbError(s.to_owned());
        let hex = s.strip_prefix('#').ok_or_else(err)?;
        if hex.len() != 6 || !hex.is_ascii() {
            return Err(err());
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| err());
        Ok(Self::new(channel(0)?, channel(2)?, channel(4)?))
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex() {
        assert_eq!("#14161B".parse(), Ok(Rgb::new(0x14, 0x16, 0x1b)));
    }

    #[test]
    fn round_trips() {
        let c = Rgb::new(1, 2, 255);
        assert_eq!(c.to_string().parse(), Ok(c));
    }

    #[test]
    fn rejects_invalid() {
        for s in ["", "141618", "#1416", "#14161g", "#14161bff", "#ééé"] {
            assert!(s.parse::<Rgb>().is_err(), "{s} should be rejected");
        }
    }
}
