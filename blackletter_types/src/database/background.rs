//! Raw 5e-database background shape (`5e-SRD-Backgrounds.json`).

#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Background {
    pub index: String,
    pub name: String,
    #[serde(default)]
    pub starting_proficiencies: Vec<Ref>,
    pub language_options: Option<LanguageOptions>,
    #[serde(default)]
    pub starting_equipment: Vec<EquipmentEntry>,
    pub starting_gold: Option<Gold>,
    pub feature: Option<Feature>,
}

/// A `{index, name, url}` reference entry.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Ref {
    pub index: String,
    pub name: String,
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct LanguageOptions {
    pub choose: i64,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct EquipmentEntry {
    pub equipment: Ref,
    pub quantity: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Gold {
    pub quantity: i64,
    pub unit: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Feature {
    pub name: String,
    pub desc: crate::database::generic::StringOrVec,
}
