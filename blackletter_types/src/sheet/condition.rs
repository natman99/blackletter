use std::{fmt::Display, str::FromStr};

#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "rhai")]
use rhai::CustomType;

use crate::{Score, database};

#[derive(Debug, Clone, PartialEq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
/// Conditions affecting the character.
pub struct Conditions {
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
/// One of the fifteen SRD conditions (PHB appendix A), or a homebrew one.
pub enum ConditionKind {
    Blinded,
    Charmed,
    Deafened,
    /// Modeled as a single condition; the six levels are tracked separately.
    Exhaustion,
    Frightened,
    Grappled,
    Incapacitated,
    Invisible,
    Paralyzed,
    Petrified,
    Poisoned,
    Prone,
    Restrained,
    Stunned,
    Unconscious,
    /// Homebrew / non-SRD condition, kept by name.
    Custom(String),
}

impl Default for ConditionKind {
    fn default() -> Self {
        Self::Custom(String::new())
    }
}

impl Display for ConditionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConditionKind::Custom(s) => write!(f, "{s}"),
            other => write!(f, "{other:?}"),
        }
    }
}

impl FromStr for ConditionKind {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "blinded" => Self::Blinded,
            "charmed" => Self::Charmed,
            "deafened" => Self::Deafened,
            "exhaustion" => Self::Exhaustion,
            "frightened" => Self::Frightened,
            "grappled" => Self::Grappled,
            "incapacitated" => Self::Incapacitated,
            "invisible" => Self::Invisible,
            "paralyzed" => Self::Paralyzed,
            "petrified" => Self::Petrified,
            "poisoned" => Self::Poisoned,
            "prone" => Self::Prone,
            "restrained" => Self::Restrained,
            "stunned" => Self::Stunned,
            "unconscious" => Self::Unconscious,
            _ => Self::Custom(s.to_string()),
        })
    }
}

impl ConditionKind {
    /// Saving throws the creature automatically fails while under this
    /// condition (Stunned/Paralyzed/Petrified/Unconscious: Str and Dex).
    #[must_use]
    pub fn auto_fail_saves(&self) -> &'static [Score] {
        match self {
            Self::Paralyzed | Self::Petrified | Self::Stunned | Self::Unconscious => {
                &[Score::Str, Score::Dex]
            }
            _ => &[],
        }
    }

    /// Saving throws made with disadvantage (Restrained: Dex).
    #[must_use]
    pub fn disadvantage_on_saves(&self) -> &'static [Score] {
        match self {
            Self::Restrained => &[Score::Dex],
            _ => &[],
        }
    }

    /// Attack rolls against the creature have advantage. Prone is
    /// conditional (only within 5 ft) and not reported here.
    #[must_use]
    pub fn attacks_against_have_advantage(&self) -> bool {
        matches!(
            self,
            Self::Blinded
                | Self::Paralyzed
                | Self::Petrified
                | Self::Restrained
                | Self::Stunned
                | Self::Unconscious
        )
    }

    /// Attack rolls against the creature have disadvantage (Invisible;
    /// Prone beyond 5 ft is conditional and not reported here).
    #[must_use]
    pub fn attacks_against_have_disadvantage(&self) -> bool {
        matches!(self, Self::Invisible)
    }

    /// The creature's own attack rolls have disadvantage.
    #[must_use]
    pub fn target_attack_rolls_disadvantaged(&self) -> bool {
        matches!(
            self,
            Self::Blinded | Self::Frightened | Self::Poisoned | Self::Prone | Self::Restrained
        )
    }

    /// The creature's ability checks have disadvantage. Blinded/Deafened
    /// auto-fail only sight/hearing-based checks, and Frightened is
    /// line-of-sight-conditional, so neither is reported here.
    #[must_use]
    pub fn ability_checks_disadvantaged(&self) -> bool {
        matches!(self, Self::Frightened | Self::Poisoned | Self::Exhaustion)
    }

    /// The creature can't take actions or reactions.
    #[must_use]
    pub fn cant_take_actions(&self) -> bool {
        matches!(
            self,
            Self::Incapacitated
                | Self::Paralyzed
                | Self::Petrified
                | Self::Stunned
                | Self::Unconscious
        )
    }

    /// The creature's speed is 0.
    #[must_use]
    pub fn speed_zero(&self) -> bool {
        matches!(
            self,
            Self::Grappled
                | Self::Paralyzed
                | Self::Petrified
                | Self::Restrained
                | Self::Stunned
                | Self::Unconscious
        )
    }

    /// Any attack that hits is a critical hit if the attacker is within
    /// 5 feet (Paralyzed, Unconscious).
    #[must_use]
    pub fn melee_hits_are_auto_critical(&self) -> bool {
        matches!(self, Self::Paralyzed | Self::Unconscious)
    }

    /// The creature's own attack rolls have advantage (Invisible).
    #[must_use]
    pub fn own_attacks_have_advantage(&self) -> bool {
        matches!(self, Self::Invisible)
    }
}

#[derive(Debug, Clone, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
/// A condition including its kind and duration.
pub struct Condition {
    pub kind: ConditionKind,
    pub desc: String,
    pub duration: i32,
}

impl Default for Condition {
    fn default() -> Self {
        Self {
            kind: ConditionKind::default(),
            desc: String::new(),
            duration: -1,
        }
    }
}

impl Display for Condition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let turns = if self.duration < 0 {
            "inf".to_string()
        } else {
            format!("{} turns", self.duration)
        };
        write!(f, "{}: {}", self.kind, turns)
    }
}

impl Condition {
    pub fn new(name: impl Into<String>, duration: i32) -> Self {
        let name = name.into();
        Self {
            kind: name.parse().unwrap_or(ConditionKind::Custom(name)),
            duration,
            desc: "".to_string(),
        }
    }

    pub fn tick(&mut self) {
        if self.duration == 0 {
            return;
        }
        self.duration -= 1;
    }

    pub fn expired(&self) -> bool {
        self.duration == 0
    }
}

impl From<database::condition::Condition> for Condition {
    fn from(value: database::condition::Condition) -> Self {
        Self {
            kind: value
                .name
                .parse()
                .unwrap_or(ConditionKind::Custom(value.name.clone())),
            desc: value.desc.join("\n"),
            duration: -1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tick_and_expiry() {
        let mut c = Condition::new("Poisoned".to_string(), 2);
        assert!(!c.expired());
        c.tick();
        assert!(!c.expired());
        c.tick();
        assert!(c.expired());
        // Ticking an expired condition stays put.
        c.tick();
        assert_eq!(c.duration, 0);
    }

    #[test]
    fn test_negative_duration_never_expires() {
        // -1 is the "until ended" sentinel; it must not expire on ticking.
        let mut c = Condition::new("Blinded".to_string(), -1);
        for _ in 0..10 {
            c.tick();
            assert!(!c.expired());
        }
    }

    #[test]
    fn test_display() {
        assert_eq!(
            Condition::new("Poisoned".to_string(), 2).to_string(),
            "Poisoned: 2 turns"
        );
        assert_eq!(
            Condition::new("Blinded".to_string(), -1).to_string(),
            "Blinded: inf"
        );
    }

    #[test]
    fn test_from_database_condition() {
        let db = database::condition::Condition {
            index: "blinded".to_string(),
            name: "Blinded".to_string(),
            desc: vec!["first".to_string(), "second".to_string()],
        };
        let c: Condition = db.into();
        assert_eq!(c.kind, ConditionKind::Blinded);
        assert_eq!(c.desc, "first\nsecond");
        assert_eq!(c.duration, -1);
    }

    #[test]
    fn test_kind_parsing() {
        assert_eq!(
            "blinded".parse::<ConditionKind>().unwrap(),
            ConditionKind::Blinded
        );
        assert_eq!(
            "Poisoned".parse::<ConditionKind>().unwrap(),
            ConditionKind::Poisoned
        );
        assert_eq!(
            "Doomed".parse::<ConditionKind>().unwrap(),
            ConditionKind::Custom("Doomed".to_string())
        );
        // Condition::new keeps SRD names typed and homebrew names Custom.
        assert_eq!(
            Condition::new("Stunned".to_string(), -1).kind,
            ConditionKind::Stunned
        );
        assert_eq!(
            Condition::new("Homebrew Doom".to_string(), 3).kind,
            ConditionKind::Custom("Homebrew Doom".to_string())
        );
    }

    #[test]
    fn test_srd_condition_effects() {
        // Wiki/12 conditions.md (PHB appendix A).
        use crate::Score;
        assert_eq!(
            ConditionKind::Stunned.auto_fail_saves(),
            [Score::Str, Score::Dex]
        );
        assert_eq!(
            ConditionKind::Unconscious.auto_fail_saves(),
            [Score::Str, Score::Dex]
        );
        assert_eq!(
            ConditionKind::Restrained.disadvantage_on_saves(),
            [Score::Dex]
        );
        assert!(ConditionKind::Blinded.attacks_against_have_advantage());
        assert!(ConditionKind::Invisible.attacks_against_have_disadvantage());
        assert!(!ConditionKind::Invisible.attacks_against_have_advantage());
        assert!(ConditionKind::Poisoned.target_attack_rolls_disadvantaged());
        assert!(ConditionKind::Incapacitated.cant_take_actions());
        assert!(ConditionKind::Grappled.speed_zero());
        assert!(!ConditionKind::Grappled.cant_take_actions());
        assert!(ConditionKind::Paralyzed.melee_hits_are_auto_critical());
        assert!(ConditionKind::Unconscious.melee_hits_are_auto_critical());
        assert!(!ConditionKind::Restrained.melee_hits_are_auto_critical());
    }
}
