#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::reference::Reference;

/// Raw `5e-SRD-Classes.json` entry — lean: only the fields Blackletter
/// reads. The old generated full-shape version never compiled against
/// real SRD JSON (externally-tagged option enums) and was unused.
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseClass {
    pub index: String,
    pub name: String,
    pub hit_die: i32,
    pub saving_throws: Vec<Reference>,
    pub proficiencies: Vec<Reference>,
    pub subclasses: Vec<Reference>,
    #[serde(default)]
    pub url: String,
}
