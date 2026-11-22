#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::reference::Reference;

/// Raw `5e-SRD-Subclasses.json` entry. (Subraces live in `race.rs`.)
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseSubclass {
    pub index: String,
    pub name: String,
    pub class: Reference,
    pub subclass_flavor: String,
    pub desc: Vec<String>,
    #[serde(default)]
    pub url: String,
}
