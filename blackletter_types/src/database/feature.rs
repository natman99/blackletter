#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::reference::Reference;

/// Raw `5e-SRD-Features.json` entry.
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseFeature {
    pub index: String,
    pub name: String,
    pub class: Option<Reference>,
    pub subclass: Option<Reference>,
    pub level: i32,
    pub prerequisites: Vec<serde_json::Value>,
    pub desc: Vec<String>,
    pub parent: Option<Reference>,
    pub reference: Option<FeatureReference>,
    pub features: Vec<Reference>,
    #[serde(default)]
    pub url: String,
}

/// `reference` is sometimes a `{index, name, url}` object and sometimes
/// a bare URL string in the SRD.
#[cfg(feature = "serde")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FeatureReference {
    Obj(Reference),
    Url(String),
}
