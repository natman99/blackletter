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

#[cfg_attr(feature = "rhai", derive(CustomType))]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Feat {
    pub name: String,
    pub desc: String,
    pub feature: Option<Feature>,
    pub source: Option<Source>,
}

impl Default for Feat {
    fn default() -> Self {
        Self {
            name: String::new(),
            desc: String::new(),
            feature: None,
            source: None,
        }
    }
}

impl HasSource for Feat {
    fn source(&self) -> Option<&Source> {
        self.source.as_ref()
    }

    fn add_source(&mut self, source: Source) {
        self.source = Some(source)
    }
}

impl Feat {
    /// Convert a raw SRD feat. Mechanical effects are not parsed from
    /// prose — the sheet's Feature stays for the table to attach.
    #[cfg(feature = "serde")]
    pub fn try_from_database(value: crate::database::feat::DatabaseFeat) -> Self {
        Self {
            name: value.name,
            desc: value.desc.join("\n"),
            feature: None,
            source: None,
        }
    }

    /// The effects this feat grants (its wrapped feature, if any).
    #[must_use]
    pub fn effects(&self) -> &[Effect] {
        self.feature
            .as_ref()
            .map(|f| f.effects.as_slice())
            .unwrap_or(&[])
    }

    /// Add an effect to this feat, creating the wrapped feature if needed.
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
