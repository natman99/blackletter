#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::reference::{Cost, Reference};

/// Raw `5e-SRD-Equipment.json` entry.
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Equipment {
    pub index: String,
    pub name: String,
    pub equipment_category: Reference,
    pub weapon_category: Option<String>,
    pub weapon_range: Option<String>,
    pub category_range: Option<String>,
    pub cost: Option<Cost>,
    pub damage: Option<Damage>,
    pub range: Option<Range>,
    pub weight: Option<f64>,
    pub properties: Vec<Reference>,
    pub desc: Vec<String>,
    pub special: Vec<String>,
    pub quantity: Option<i32>,
    pub throw_range: Option<Range>,
    pub armor_category: Option<String>,
    pub armor_class: Option<ArmorClass>,
    pub str_minimum: Option<i32>,
    pub stealth_disadvantage: Option<bool>,
    pub property: Option<Reference>,
    pub tool_type: Option<String>,
    pub contents: Vec<ContentEntry>,
}

#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Damage {
    pub damage_dice: Option<String>,
    pub damage_type: Option<Reference>,
}

#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Range {
    pub normal: Option<i32>,
    #[serde(default)]
    pub long: Option<i32>,
}

/// Armor entries carry `armor_class: { base, dex_bonus, max_bonus? }`.
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ArmorClass {
    pub base: i32,
    pub dex_bonus: bool,
    #[serde(default)]
    pub max_bonus: Option<i32>,
}

/// Pack contents: `{ item, quantity }`.
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContentEntry {
    pub item: Reference,
    pub quantity: i32,
}
