#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::reference::Reference;

/// Raw `5e-SRD-Feats.json` entry.
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseFeat {
    pub index: String,
    pub name: String,
    pub prerequisites: Vec<Prerequisite>,
    pub desc: Vec<String>,
    #[serde(default)]
    pub url: String,
}

#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Prerequisite {
    pub ability_score: Reference,
    pub minimum_score: i32,
}
