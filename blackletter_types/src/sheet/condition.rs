use std::fmt::Display;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "rhai")]
use rhai::CustomType;

use crate::database;

#[derive(Debug, Clone, PartialEq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
/// Conditions affecting the character.
pub struct Conditions {
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
/// A condition including a name and duration.
pub struct Condition {
    pub name: String,
    pub desc: String,
    pub duration: i32,
}

impl Default for Condition {
    fn default() -> Self {
        Self {
            name: "Stunned".to_string(),
            desc: "".to_string(),
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
        write!(f, "{}: {}", self.name, turns)
    }
}

impl Condition {
    pub fn new(name: String, duration: i32) -> Self {
        Self {
            name,
            duration,
            desc: "".to_string(),
        }
    }

    pub fn tick(&mut self) {
        if self.duration == 0 {
            return;
        }
        self.duration += -1;
    }

    pub fn expired(&self) -> bool {
        self.duration == 0
    }
}

impl From<database::condition::Condition> for Condition {
    fn from(value: database::condition::Condition) -> Self {
        Self {
            name: value.name,
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
        assert_eq!(c.name, "Blinded");
        assert_eq!(c.desc, "first\nsecond");
        assert_eq!(c.duration, -1);
    }
}
