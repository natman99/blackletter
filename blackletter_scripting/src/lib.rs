use std::{
    fmt::Debug,
    sync::{Arc, LazyLock, Mutex},
};

use blackletter_types::{
    AbilityScores, ArmorClass, Character, HitPoints, Initiative, Inventory, WalkingSpeed,
    ability_score::AbilityScore,
    class::{Class, Classes},
    condition::Conditions,
    feat::Feat,
    feature::Features,
    language::Languages,
    lore::Lore,
    meter::Meters,
    senses::Senses,
    skills::Skills,
    spells::{Spell, Spells},
    traits::Trait,
};
use regex::Regex;
use rhai::{Dynamic, Engine, Module, Scope, combine_with_exported_module};

#[cfg(test)]
mod tests;

mod library;

pub mod registry;
pub use registry::Registry;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord)]
pub enum Command {
    FixedHp,
}

#[derive(Debug)]
pub struct Interface {
    engine: Engine,
    pub spells: Spells,
    pub conditions: Spells,
    pub result: Arc<Mutex<String>>,
}

impl Default for Interface {
    fn default() -> Self {
        Self::new()
    }
}

impl Interface {
    #[must_use]
    pub fn new() -> Self {
        let mut engine = Engine::new();
        let result = Arc::new(Mutex::new(String::new()));

        let r = result.clone();

        engine.on_print(move |f| r.lock().unwrap().push_str(f));

        register_engine_types(&mut engine);

        register_engine_functions(&mut engine);

        Self {
            engine,
            spells: Default::default(),
            conditions: Default::default(),
            result,
        }
    }

    pub fn execute(
        &mut self,
        script: &str,
        character: Option<Character>,
    ) -> anyhow::Result<Character> {
        let mut scope = Scope::new();

        self.inject_scope(&mut scope);

        let _: () = self.engine.eval_with_scope(&mut scope, script)?;

        let mut c = self.extract_scope(&mut scope)?;

        if let Some(char) = character {
            c.health.current = char.health.current;
            for (k, i) in &char.meters.meters {
                if let Some(meter) = c.meters.meters.get_mut(k) {
                    meter.spent = i.spent;
                }
            }
            for i in char.conditions.conditions {
                c.conditions.conditions.push(i);
            }
        }

        Ok(c)
    }
    /// Create default scope.
    fn inject_scope(&self, scope: &mut Scope) {
        let Character {
            name,
            race,
            gold,
            classes,
            initiative,
            armor_class,
            health,
            inventory,
            ability_scores,
            skills,
            saving_throws,
            spells,
            senses,
            features,
            feats,
            traits,
            ability_modifier,
            meters,
            lore,
            walking_speed,
            languages,
            conditions,
            death_saves,
            exhaustion,
            spell_slots_override,
            needs_spell_prep,
        } = Character::default();

        let armor_class = armor_class.flat;
        let walking_speed = walking_speed.0;

        scope.push("name", name);
        match race {
            Some(r) => scope.push("race", r),
            None => scope.push("race", ()),
        };
        scope.push("gold", gold);
        scope.push("classes", classes);
        scope.push("initiative", initiative);
        match armor_class {
            Some(v) => scope.push("armor_class", v),
            None => scope.push("armor_class", ()),
        };
        scope.push("health", health);
        scope.push("inventory", inventory);
        scope.push("ability_scores", ability_scores);
        scope.push("skills", skills);
        scope.push("spells", spells);
        scope.push("senses", senses);
        scope.push("features", features);
        scope.push("feats", feats);
        scope.push("traits", traits);
        scope.push("ability_modifier", ability_modifier);
        scope.push("meters", meters);
        scope.push("lore", lore);
        scope.push("walking_speed", walking_speed);
        scope.push("languages", languages);
        scope.push("conditions", conditions);
        scope.push("saving_throws", saving_throws);
        scope.push("death_saves", death_saves);
        scope.push("exhaustion", exhaustion as i64);
        match spell_slots_override {
            Some(v) => scope.push("spell_slots_override", v),
            None => scope.push("spell_slots_override", ()),
        };
        scope.push("needs_spell_prep", needs_spell_prep);

        // We should run automated additions including adding meters, abilities, calculating health, and applying item stats.
        scope.push("calculate", true);
    }
    /// Run code against a character and return the output.
    pub fn interactive(&self, script: &str, character: Character) -> anyhow::Result<Dynamic> {
        const RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s?(\d+d\d+)\s?").unwrap());

        let script = RE.replace_all(script, " roll(\"$1\").roll()");

        println!("{script}");

        let mut scope = Scope::new();
        self.inject_known_scope(&mut scope, character);

        let r: Dynamic = self
            .engine
            .eval_expression_with_scope(&mut scope, &script)?;
        Ok(r)
    }
    /// Create scope from a character.
    fn inject_known_scope(&self, scope: &mut Scope, character: Character) {
        let Character {
            name,
            race,
            gold,
            classes,
            initiative,
            armor_class,
            health,
            inventory,
            ability_scores,
            skills,
            saving_throws,
            spells,
            senses,
            features,
            feats,
            traits,
            ability_modifier,
            meters,
            lore,
            walking_speed,
            languages,
            conditions,
            death_saves,
            exhaustion,
            spell_slots_override,
            needs_spell_prep,
        } = character;

        let armor_class = armor_class.flat;
        let walking_speed = walking_speed.0;

        scope.push("str", ability_scores.str);
        scope.push("dex", ability_scores.dex);
        scope.push("con", ability_scores.con);
        scope.push("wis", ability_scores.wis);
        scope.push("int", ability_scores.int);
        scope.push("cha", ability_scores.cha);

        scope.push("name", name);
        match race {
            Some(r) => scope.push("race", r),
            None => scope.push("race", ()),
        };
        scope.push("gold", gold);
        scope.push("classes", classes);
        scope.push("initiative", initiative);
        match armor_class {
            Some(v) => scope.push("armor_class", v),
            None => scope.push("armor_class", ()),
        };
        scope.push("health", health);
        scope.push("inventory", inventory);
        scope.push("ability_scores", ability_scores);
        scope.push("skills", skills);
        scope.push("spells", spells);
        scope.push("senses", senses);
        scope.push("features", features);
        scope.push("feats", feats);
        scope.push("traits", traits);
        scope.push("ability_modifier", ability_modifier);
        scope.push("meters", meters);
        scope.push("lore", lore);
        scope.push("walking_speed", walking_speed);
        scope.push("languages", languages);
        scope.push("conditions", conditions);
        scope.push("saving_throws", saving_throws);
        scope.push("death_saves", death_saves);
        scope.push("exhaustion", exhaustion as i64);
        match spell_slots_override {
            Some(v) => scope.push("spell_slots_override", v),
            None => scope.push("spell_slots_override", ()),
        };
        scope.push("needs_spell_prep", needs_spell_prep);
    }

    fn extract_scope(&self, scope: &mut Scope) -> anyhow::Result<Character> {
        macro_rules! extract {
            ($key:expr, $label:expr) => {{
                let var = scope
                    .get_mut($key)
                    .ok_or_else(|| anyhow::anyhow!("missing variable: {}", $label))?;
                var.take()
                    .try_cast()
                    .ok_or_else(|| anyhow::anyhow!("failed to cast variable: {}", $label))
            }};
        }
        // rhai represents `None` as `()` but won't cast a present value
        // back into `Option<T>`, so optional variables need a unit check
        // before the cast.
        macro_rules! extract_opt {
            ($key:expr, $label:expr, $ty:ty) => {{
                let var = scope
                    .get_mut($key)
                    .ok_or_else(|| anyhow::anyhow!("missing variable: {}", $label))?;
                let value = var.take();
                if value.is_unit() {
                    Ok::<_, anyhow::Error>(None)
                } else {
                    Ok(Some(value.try_cast::<$ty>().ok_or_else(|| {
                        anyhow::anyhow!("failed to cast variable: {}", $label)
                    })?))
                }
            }};
        }

        let name = extract!("name", "name")?;
        let race = extract_opt!("race", "race", blackletter_types::race::Race)?;
        let gold = extract!("gold", "gold")?;
        let classes = extract!("classes", "classes")?;
        let initiative = extract!("initiative", "initiative")?;
        let armor_class = ArmorClass {
            flat: extract_opt!("armor_class", "armor_class", i64)?,
        };

        let health = extract!("health", "health")?;
        let inventory = extract!("inventory", "inventory")?;
        let ability_scores = extract!("ability_scores", "ability_scores")?;
        let skills = extract!("skills", "skills")?;
        let spells = extract!("spells", "spells")?;
        let senses = extract!("senses", "senses")?;
        let features = extract!("features", "features")?;
        let feats = extract!("feats", "feats")?;
        let traits = extract!("traits", "traits")?;
        let ability_modifier = extract!("ability_modifier", "ability_modifier")?;
        let meters = extract!("meters", "meters")?;
        let lore = extract!("lore", "lore")?;
        let walking_speed_raw: i64 = extract!("walking_speed", "walking_speed")?;
        let walking_speed = WalkingSpeed(walking_speed_raw);
        let languages = extract!("languages", "languages")?;
        let conditions = extract!("conditions", "conditions")?;
        let saving_throws = extract!("saving_throws", "saving_throws")?;
        let death_saves = extract!("death_saves", "death_saves")?;
        let exhaustion_raw: i64 = extract!("exhaustion", "exhaustion")?;
        let exhaustion = exhaustion_raw as u8;
        let spell_slots_override =
            extract_opt!("spell_slots_override", "spell_slots_override", Vec<i64>)?;
        let needs_spell_prep = extract!("needs_spell_prep", "needs_spell_prep")?;

        Ok(Character {
            needs_spell_prep,
            name,
            race,
            gold,
            classes,
            initiative,
            armor_class,
            health,
            inventory,
            ability_scores,
            skills,
            spells,
            senses,
            features,
            feats,
            traits,
            ability_modifier,
            meters,
            lore,
            walking_speed,
            languages,
            conditions,
            saving_throws,
            death_saves,
            exhaustion,
            spell_slots_override,
        })
    }
}

fn register_engine_types(engine: &mut Engine) {
    engine.build_type::<Character>();
    engine.build_type::<AbilityScores>();
    engine.build_type::<AbilityScore>();
    engine.build_type::<ArmorClass>();
    engine.build_type::<HitPoints>();
    engine.build_type::<Initiative>();
    engine.build_type::<Inventory>();
    engine.build_type::<WalkingSpeed>();
    engine.build_type::<Classes>();
    engine.build_type::<Class>();
    engine.build_type::<Conditions>();
    engine.build_type::<Features>();
    engine.build_type::<Languages>();
    engine.build_type::<Lore>();
    engine.build_type::<Meters>();
    engine.build_type::<Senses>();
    engine.build_type::<Skills>();
    engine.build_type::<Spells>();
    engine.build_type::<Spell>();
    engine.build_type::<Feat>();
    engine.build_type::<Trait>();
    engine.build_type::<blackletter_types::death::DeathSaves>();
    engine.build_type::<blackletter_types::race::Race>();
    engine.register_fn("Race", |name: rhai::ImmutableString| {
        blackletter_types::race::Race::new(&name)
    });

    engine.register_indexer_get_set(Classes::get, Classes::set);
}

fn register_engine_functions(engine: &mut Engine) {
    let mut module = Module::new();
    combine_with_exported_module!(&mut module, "blackletter", library::library);
    engine.register_global_module(module.into());
}
