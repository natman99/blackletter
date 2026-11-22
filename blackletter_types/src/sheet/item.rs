#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "rhai")]
use rhai::CustomType;

use crate::{
    AbilityScores, Score,
    feature::{Effect, Feature},
    roll::Rollable,
    sheet::{
        roll::Roll,
        source::{HasSource, Source},
    },
    skills::Skills,
};

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
/// An item.
pub struct Item {
    /// The name of the item
    pub name: String,
    /// The description of the item.
    pub description: String,
    /// How many of the item.
    pub quantity: i32,
    /// The roll if the item deals damage or has an effect.
    pub roll: Option<Roll>,
    pub to_hit: Option<Score>,
    pub source: Option<Source>,
    pub feature: Option<Feature>,
    /// Whether the item is worn/held in a way that makes its feature's
    /// effects count. Only equipped items contribute to `Modifiers`.
    pub equipped: bool,
}

impl Default for Item {
    fn default() -> Self {
        Self {
            name: "Example item".to_string(),
            description: "Example description".to_string(),
            roll: None,
            quantity: 1,
            source: None,
            feature: None,
            to_hit: None,
            equipped: false,
        }
    }
}

impl Item {
    /// Convert a raw SRD equipment entry into a sheet item: name, joined
    /// description, and the damage roll when the entry has one.
    #[cfg(feature = "serde")]
    pub fn try_from_database(value: crate::database::item::Equipment) -> anyhow::Result<Self> {
        use std::str::FromStr;
        let roll = value
            .damage
            .as_ref()
            .and_then(|d| d.damage_dice.as_ref())
            .map(|dice| Roll::from_str(dice))
            .transpose()?;
        let mut description = value.desc.join("\n");
        if !value.special.is_empty() {
            if !description.is_empty() {
                description.push('\n');
            }
            description.push_str(&value.special.join("\n"));
        }
        Ok(Self {
            name: value.name,
            description,
            quantity: 1,
            roll,
            to_hit: None,
            source: None,
            feature: None,
            equipped: false,
        })
    }

    pub fn with_roll(mut self, roll: Roll) -> Self {
        self.roll = Some(roll);
        self
    }

    pub fn with_desc(mut self, description: String) -> Self {
        self.description = description;
        self
    }

    pub fn with_quantity(mut self, quantity: i32) -> Self {
        self.quantity = quantity;
        self
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name = name;
        self
    }

    pub fn new(name: String) -> Self {
        Self {
            name,
            ..Default::default()
        }
    }

    pub fn to_hit(&self, scores: &AbilityScores, skills: &Skills) -> Option<i64> {
        if let Some(s) = self.to_hit {
            Some(scores.get(&s).modifier() + skills.proficiency_bonus)
        } else {
            None
        }
    }

    /// The effects this item grants (its feature, if any).
    #[must_use]
    pub fn effects(&self) -> &[Effect] {
        self.feature
            .as_ref()
            .map(|f| f.effects.as_slice())
            .unwrap_or(&[])
    }
}

impl Rollable for Item {
    fn roll(
        &self,
        rng: &mut impl rand::Rng,
        special: Option<super::roll::RollModifier>,
    ) -> Option<super::roll::RollResult> {
        self.roll.as_ref().map(|r| r.roll(rng, special))
    }
}

impl HasSource for Item {
    fn source(&self) -> Option<&Source> {
        self.source.as_ref()
    }

    fn add_source(&mut self, source: Source) {
        self.source = Some(source)
    }
}

impl Item {
    pub fn add(&mut self, feature: Effect) {
        if self.feature.is_none() {
            self.feature = Some(Feature {
                name: self.name.clone(),
                source: self.source().cloned(),
                effects: vec![],
            });
        }

        self.feature.as_mut().unwrap().effects.push(feature);
    }
}
