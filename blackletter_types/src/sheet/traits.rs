use crate::{
    feature::{Effect, Feature},
    sheet::source::{HasSource, Source},
};

#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "rhai")]
use rhai::CustomType;

/// A trait granted to the sheet (race traits like Darkvision, class
/// traits, or homebrew). Mirrors `Feat`: descriptive text plus an optional
/// mechanical `Feature`. Database traits can be imported into this shape.
#[cfg_attr(feature = "rhai", derive(CustomType))]
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Trait {
    pub name: String,
    pub desc: String,
    pub feature: Option<Feature>,
    pub source: Option<Source>,
}

impl HasSource for Trait {
    fn source(&self) -> Option<&Source> {
        self.source.as_ref()
    }

    fn add_source(&mut self, source: Source) {
        self.source = Some(source)
    }
}

impl Trait {
    /// The effects this trait grants (its feature, if any).
    #[must_use]
    pub fn effects(&self) -> &[Effect] {
        self.feature
            .as_ref()
            .map(|f| f.effects.as_slice())
            .unwrap_or(&[])
    }

    /// Add an effect, creating the wrapped feature if needed.
    pub fn add(&mut self, effect: Effect) {
        self.feature.get_or_insert_with(|| Feature {
            name: self.name.clone(),
            source: self.source.clone(),
            effects: vec![],
        });
        if let Some(feature) = &mut self.feature {
            feature.effects.push(effect);
        }
    }
}
