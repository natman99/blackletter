//! Death saving throws (SRD 2014, Wiki/07 combat.md:440-452).

#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "rhai")]
use rhai::CustomType;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
pub struct DeathSaves {
    pub successes: i32,
    pub failures: i32,
    /// SRD: death saves aren't tied to an ability score — "aided only by
    /// spells and features". This bonus is that seam (plus house rules).
    pub bonus: i64,
    pub stable: bool,
}

/// What a death saving throw resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum DeathSaveOutcome {
    Success,
    Failure,
    /// Third success: stable, unconscious, no more saves.
    Stable,
    /// Third failure (or massive damage): dead.
    Dead,
    /// Natural 20: regain 1 hit point, saves reset.
    Revived,
}
