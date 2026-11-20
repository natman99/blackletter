use blackletter_types::{
    Item, database::spell::DatabaseSpell, feature::Feature, spells::Spell,
};

use crate::{
    App,
    command::{self, CommandError},
};

pub fn create(cmd: &str, app: &mut App) -> command::Result<()> {
    let split = cmd.split(" ").collect::<Vec<&str>>();

    if let Some(kind) = split.get(1) {
        if ["spell", "feature", "item"].contains(kind) {
            let res = std::process::Command::new("blackletter_ui").arg(kind).output();
            match res {
                Ok(res) => {
                    let s = match String::from_utf8(res.stdout) {
                        Ok(s) => s,
                        Err(a) => return Err(CommandError::CommandError(Box::new(a))),
                    };
                    let s = s.trim();
                    match *kind {
                        "spell" => {
                            let j = serde_json::from_str::<DatabaseSpell>(&s);
                            match j {
                                Ok(spell) => {
                                    let name = spell.name.clone();
                                    let spell = Spell::try_from_database(spell, &app.sheet)
                                        .map_err(|e| CommandError::CommandError(e.into_boxed_dyn_error()))?;
                                    app.sheet.spells.spells.insert(name, spell);
                                }
                                Err(a) => return Err(CommandError::CommandError(Box::new(a))),
                            }
                        }

                        "item" => {
                            let j = serde_json::from_str::<Item>(&s);
                            match j {
                                Ok(item) => {
                                    app.sheet.inventory.0.insert(item.name.to_string(), item);
                                }
                                Err(a) => return Err(CommandError::CommandError(Box::new(a))),
                            }
                        }

                        "feature" => {
                            let j = serde_json::from_str::<Feature>(&s);
                            match j {
                                Ok(feature) => {
                                    app.sheet.features.inner.push(feature);
                                }
                                Err(a) => return Err(CommandError::CommandError(Box::new(a))),
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                Err(e) => return Err(CommandError::CommandError(Box::new(e))),
            }
        } else {
            return Err(CommandError::ArgumentError);
        }
    }

    Ok(())
}
