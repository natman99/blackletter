use std::fmt::Display;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// The rules edition a corpus entry belongs to. The table's rulings are
/// 2014; 2024 data must never leak in untagged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Edition {
    /// D&D 2014 (PHB-era SRD). The default.
    #[default]
    Y2014,
    /// D&D 2024 — only ever present if explicitly loaded.
    Y2024,
}

impl Display for Edition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Edition::Y2014 => write!(f, "2014"),
            Edition::Y2024 => write!(f, "2024"),
        }
    }
}

/// The `{ index, name, url }` reference shape shared by every SRD file.
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    pub index: String,
    pub name: String,
    #[serde(default)]
    pub url: String,
}

#[cfg(feature = "serde")]
impl Reference {
    pub fn new(index: &str, name: &str) -> Self {
        Self {
            index: index.to_string(),
            name: name.to_string(),
            url: String::new(),
        }
    }
}

/// `{ quantity, unit }` cost shape.
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    pub quantity: f64,
    pub unit: String,
}
