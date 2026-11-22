#[cfg(feature = "schema")]
use schemars::JsonSchema;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "rhai")]
use rhai::CustomType;

use strum::{Display, EnumString};

use crate::{
    Score,
    class::ClassType,
    damage::DamageType,
    roll::{Roll, Rollable},
    sheet::source::{HasSource, Source},
};

#[cfg(feature = "serde")]
use crate::database::spell::DatabaseSpell;

#[cfg(feature = "serde")]
use crate::Character;

#[cfg(feature = "serde")]
use std::str::FromStr;

#[cfg(feature = "serde")]
use anyhow::Context;

/// A spell's material component: what it is, what it costs (gp), and
/// whether casting consumes it (PHB 2014: consumed only when a spell
/// says so, e.g. Identify's 100 gp pearl).
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MaterialComponent {
    /// Description from the spell text ("a pearl worth at least 100 gp").
    pub name: String,
    /// Cost in gold pieces. 0 = covered by a component pouch.
    pub cost: i64,
    /// Whether casting consumes the component.
    pub consumed: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "rhai", derive(CustomType))]
pub struct Spell {
    /// The name of the spell.
    pub name: String,
    /// The spell's description.
    pub desc: String,
    /// THe spell's range.
    pub range: String,
    /// The spell's casting time.
    pub cast_time: String,
    /// If the spell is concentration.
    pub concentration: bool,
    /// The duration of the spell.
    pub duration: String,
    /// If the spell can be cast as a ritual.
    pub ritual: bool,
    /// The spell's level
    pub level: i64,
    /// Whether this spell is prepared right now. Meaningful only for
    /// prepared casters (`Character::casting_mechanic`); SRD
    /// "Known and Prepared Spells" (`Wiki/08 spellcasting.md:23`).
    /// `serde(default)` keeps pre-existing saves loadable.
    #[cfg_attr(feature = "serde", serde(default))]
    pub prepared: bool,
    /// The type of the spell.
    pub spell_type: SpellType,
    /// The effect of the spell.
    pub effect: Effect,
    /// The spell's school,
    pub school: School,
    /// Optional: The area that the spell affects.
    pub area: Option<Area>,
    /// Components that the spell requires.
    pub components: Vec<Component>,
    /// Material component details (name, cost in gp, whether casting
    /// consumes it). `None` = no material component. Costs default to 0
    /// when imported (the SRD JSON has no cost field): fill in by hand or
    /// via homebrew; `Character::pay_material_cost` deducts from gold.
    pub material: Option<MaterialComponent>,
    /// The source of the spell.
    pub source: Option<Source>,
    /// The classes that can have this spell.
    pub classes: Vec<ClassType>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Area {
    pub shape: String,
    pub size: i32,
}

#[derive(Debug, Clone, PartialEq, Display)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum Effect {
    /// The spell deals damage.
    Damage(Damage),
    /// The spell heals.
    Heal(Heal),
    /// The spell has some other effect.
    Other,
}

impl Default for Effect {
    fn default() -> Self {
        use std::str::FromStr;
        Self::Damage(Damage {
            damage_type: DamageType::Force,
            damage: Roll::from_str("1d4").expect("Should never fail"),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Display)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum Heal {
    /// The spell heals a static amount.
    Static(String),
    /// The spell heals a rolled amount.
    Roll(Roll),
}

#[derive(EnumString, Display, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum Component {
    #[strum(serialize = "V")]
    /// Verbal component. Deserialized as "V".
    Verbal,
    #[strum(serialize = "S")]
    /// Semantic component. Deserialized as "S".
    Semantic,
    #[strum(serialize = "M")]
    /// Material component. Deserialized as "M".
    Material,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Damage {
    /// The damage type of the spell.
    pub damage_type: DamageType,
    /// The damage roll
    pub damage: Roll,
}

#[derive(Debug, Clone, PartialEq, EnumString, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum SpellType {
    /// Saving throw
    Saving { dc_type: Score, success: Success },
    /// Ranged attack roll
    #[default]
    Ranged,
    /// Melee attack roll
    Melee,
    /// Automatic success (no attack roll)
    Automatic,
}

/// Effect on DC success
#[derive(Debug, Clone, PartialEq, EnumString, Display, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[strum(ascii_case_insensitive)]
pub enum Success {
    #[default]
    /// Half damage
    Half,
    /// No damage
    None,
    /// Some other effect (make sure note is in description)
    Other,
}

#[derive(Debug, Clone, PartialEq, EnumString, Display, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[strum(ascii_case_insensitive)]
pub enum School {
    Conjuration,
    Necromancy,
    #[default]
    Evocation,
    Abjuration,
    Transmutation,
    Divination,
    Enchantment,
    Illusion,
    Chronurgy,
}

impl Rollable for Spell {
    fn roll(
        &self,
        rng: &mut impl rand::Rng,
        special: Option<crate::roll::RollModifier>,
    ) -> Option<crate::roll::RollResult> {
        Some(match &self.effect {
            Effect::Damage(damage) => damage.damage.roll(rng, special),
            Effect::Heal(heal) => match heal {
                Heal::Static(_) => return None,
                Heal::Roll(roll) => roll.roll(rng, special),
            },
            Effect::Other => return None,
        })
    }
}

impl HasSource for Spell {
    fn source(&self) -> Option<&Source> {
        self.source.as_ref()
    }

    fn add_source(&mut self, source: Source) {
        self.source = Some(source)
    }
}

impl Spell {
    #[cfg(feature = "serde")]
    pub fn try_from_database(value: DatabaseSpell, sheet: &Character) -> anyhow::Result<Self> {
        let material = value.material.map(|m| MaterialComponent {
            name: m,
            // The SRD JSON carries no cost; set it by hand or homebrew.
            cost: 0,
            consumed: false,
        });
        let mut components = vec![];
        for i in value.components {
            let a = Component::from_str(&i).context("Missing component")?;
            components.push(a);
        }

        let effect = {
            if let Some(heal) = value.heal_at_slot_level {
                let heal = heal.n1.or_else(|| {
                    heal.n2.or_else(|| {
                        heal.n3.or_else(|| {
                            heal.n4.or_else(|| {
                                heal.n5.or_else(|| {
                                    heal.n6.or_else(|| heal.n7.or_else(|| heal.n8.or(heal.n9)))
                                })
                            })
                        })
                    })
                });
                let Some(heal) = heal else {
                    return Err(anyhow::anyhow!("Healing missing"));
                };

                let modifier = sheet.ability_modifier;
                let modifier = sheet.ability_scores.get(&modifier).get();
                let heal = heal.replace("MOD", &modifier.to_string());
                let heal = if let Ok(roll) = Roll::from_str(&heal) {
                    Heal::Roll(roll)
                } else {
                    Heal::Static(heal)
                };
                Effect::Heal(heal)
            } else if let Some(damage) = value.damage {
                let damage_type: DamageType = if let Some(t) = damage.damage_type
                    && let Ok(d) = DamageType::from_str(&t.index)
                {
                    d
                } else {
                    DamageType::None
                };

                let roll = if let Some(a) = damage.damage_at_character_level {
                    // SRD: scales with total character level at thresholds
                    // 1, 5, 11, 17 (levels 14-16 use the 11 bucket, 20+ the 17).
                    let d = match sheet.classes.total_level().max(1) {
                        1..=4 => a.n1,
                        5..=10 => a.n5,
                        11..=16 => a.n11,
                        _ => a.n17,
                    };
                    let r = Roll::from_str(&d).context("Failed to parse character damage roll")?;
                    Some(r)
                } else if let Some(a) = damage.damage_at_slot_level {
                    let d = a.n1.or_else(|| {
                        a.n2.or_else(|| {
                            a.n3.or_else(|| {
                                a.n4.or_else(|| {
                                    a.n5.or_else(|| a.n6.or_else(|| a.n7.or_else(|| a.n8.or(a.n9))))
                                })
                            })
                        })
                    });
                    let Some(d) = d else {
                        return Err(anyhow::anyhow!("Damage not found"));
                    };
                    Some(Roll::from_str(&d).context("Failed to parse slot damage roll")?)
                } else {
                    None
                };
                let Some(roll) = roll else {
                    return Err(anyhow::anyhow!("Roll not found"));
                };
                let damage = Damage {
                    damage_type,
                    damage: roll,
                };
                Effect::Damage(damage)
            } else {
                Effect::Other
            }
        };

        let spell_type = {
            if let Some(dc) = value.dc {
                let dc_type =
                    Score::from_str(&dc.dc_type.index).context("Failed to parse score")?;
                let success =
                    Success::from_str(&dc.dc_success).context("Failed to parse success")?;
                SpellType::Saving { dc_type, success }
            } else if let Some(attack_type) = value.attack_type
                && let Ok(attack) = SpellType::from_str(&attack_type)
            {
                attack
            } else {
                SpellType::Automatic
            }
        };

        let classes = value
            .classes
            .iter()
            .map(|f| ClassType::from(f.index.as_str()))
            .collect();

        let s = Self {
            prepared: false,
            name: value.name,
            desc: value.desc.join("\n"),
            range: value.range,
            cast_time: value.casting_time,
            concentration: value.concentration,
            duration: value.duration,
            ritual: value.ritual,
            level: value.level,
            spell_type,
            effect,
            school: School::from_str(&value.school.name).context("Failed to parse school")?,
            area: if let Some(a) = value.area_of_effect {
                Some(Area {
                    shape: a.type_field,
                    size: a.size as i32,
                })
            } else {
                None
            },
            components,
            material,
            source: None,
            classes,
        };

        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "serde")]
    use std::fs;
    use std::str::FromStr;

    use super::*;

    #[test]
    fn test_school() {
        School::from_str("Conjuration").unwrap();
        let s = School::from_str("conjuration").unwrap();
        assert_eq!(s.to_string(), "Conjuration");
    }
    #[cfg(feature = "serde")]
    #[test]
    fn test_spell_from() {
        let path = "../srd/2014/en/5e-SRD-Spells.json";

        let s = fs::read_to_string(path).unwrap();
        let spells: Vec<DatabaseSpell> = serde_json::from_str(&s).unwrap();
        let sheet = Character::default();
        for i in spells {
            let name = i.name.clone();
            Spell::try_from_database(i, &sheet)
                .context(format!("{}", name))
                .unwrap();
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_cantrip_damage_scales_with_total_level() {
        use crate::class::{Class, ClassType, Level};

        let path = "../srd/2014/en/5e-SRD-Spells.json";
        let s = fs::read_to_string(path).unwrap();
        let spells: Vec<DatabaseSpell> = serde_json::from_str(&s).unwrap();
        let firebolt = spells
            .iter()
            .find(|sp| sp.index == "fire-bolt")
            .unwrap()
            .clone();

        // SRD: fire bolt damage die doubles at character levels 5, 11, 17.
        for (level, expected) in [
            (1, "1d10"),
            (4, "1d10"),
            (5, "2d10"),
            (11, "3d10"),
            (14, "3d10"), // previously panicked here
            (16, "3d10"), // previously panicked here
            (17, "4d10"),
            (20, "4d10"), // previously panicked here
        ] {
            let mut sheet = Character::default();
            sheet.classes[0].level = Level(level);
            let spell = Spell::try_from_database(firebolt.clone(), &sheet).unwrap();
            let Effect::Damage(d) = spell.effect else {
                panic!("expected damage effect at level {level}");
            };
            assert_eq!(d.damage, Roll::from_str(expected).unwrap(), "level {level}");
        }

        // Multiclass scaling uses total character level: fighter 3 / wizard 2 = 5.
        let mut sheet = Character::default();
        sheet.classes[0].level = Level(3);
        let mut wiz = Class::default();
        wiz.class = ClassType::Wizard;
        wiz.level = Level(2);
        sheet.classes.classes.push(wiz);
        let spell = Spell::try_from_database(firebolt, &sheet).unwrap();
        let Effect::Damage(d) = spell.effect else {
            panic!("expected damage effect");
        };
        assert_eq!(d.damage, Roll::from_str("2d10").unwrap());
    }
}
