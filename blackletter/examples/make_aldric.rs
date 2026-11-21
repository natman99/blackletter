//! Build Aldric Underwood (Endara campaign) from the ZenNotes character
//! note and write him into the campaign's `characters/` dir as a
//! blackletter sheet file. Run: `cargo run -p blackletter --example make_aldric`.
//!
//! Mapping rules (from `Endara/Characters/Aldric Underwood.md`):
//! - Ability scores are the note's FINAL values (19 Int already includes
//!   the Keen Mind +1); the race/background carry no mechanical bonuses so
//!   nothing double-counts. The note never states a race — left unset.
//! - The note's save bonuses (+5 CON / +7 CHA) are stale (PB +2 era);
//!   the engine derives CON/CHA proficient at PB +3 instead.
//! - Perception +6 in the note implies proficiency, but the note's own
//!   skill list doesn't include it — left unproficient, flagged to the user.

use blackletter_types::{
    AbilityScores, Background, Character, HitPoints, Inventory, Item, Score,
    ability_score::AbilityScore,
    class::ClassType,
    language::{Language, Languages},
    lore::Lore,
    meter::{Meter, RestoreTime},
    skills::Skill,
    spells::Spell,
};

fn spell(
    name: &str,
    level: i64,
    school: blackletter_types::spells::School,
    concentration: bool,
) -> Spell {
    Spell {
        name: name.into(),
        level,
        school,
        concentration,
        ..Spell::default()
    }
}

fn item(name: &str, quantity: i32) -> Item {
    Item {
        name: name.into(),
        quantity,
        ..Item::default()
    }
}

fn main() {
    let scores = AbilityScores {
        str: AbilityScore::new(12, 0),
        dex: AbilityScore::new(16, 0),
        con: AbilityScore::new(16, 0),
        int: AbilityScore::new(19, 0),
        wis: AbilityScore::new(16, 0),
        cha: AbilityScore::new(20, 0),
    };
    let mut c = Character::new(
        "Aldric Underwood".into(),
        5,
        HitPoints {
            current: 34,
            max: 34,
            level_bonus: 0,
            bonus: 0,
        },
        ClassType::Sorcerer,
        scores,
    );
    c.classes.classes[0].subclass = "Clockwork Soul".into();
    c.ability_modifier = Score::Cha;

    // Skills: class (Sorcerer: Deception, Persuasion) + background-ish
    // (Arcana, Investigation) per the note's list.
    for s in [
        Skill::Arcana,
        Skill::Deception,
        Skill::Investigation,
        Skill::Persuasion,
    ] {
        c.skills.proficiencies.toggle(s);
    }
    c.saving_throws.con = true;
    c.saving_throws.cha = true;

    // Custom background straight from the note.
    c.background = Some(Background {
        name: "Custom (House Underwood)".into(),
        skill_proficiencies: vec![Skill::Investigation, Skill::Deception],
        languages: vec![Language::Elvish, Language::Dwarvish],
        language_choices: None,
        feature: None,
        equipment: Vec::new(),
        gold: 0,
        source: None,
    });
    c.languages = Languages {
        languages: vec![
            Language::Common,
            Language::Dwarvish,
            Language::Elvish,
            Language::Primordial,
        ],
    };

    // Spells known (strikethroughs in the note are swapped-out: excluded).
    use blackletter_types::spells::School::*;
    let known = [
        spell("Fire Bolt", 0, Evocation, false),
        spell("Prestidigitation", 0, Conjuration, false),
        spell("Mage Hand", 0, Conjuration, false),
        spell("Mind Sliver", 0, Enchantment, false),
        spell("Minor Illusion", 0, Illusion, false),
        spell("Silvery Barbs", 1, Enchantment, false),
        spell("Absorb Elements", 1, Abjuration, true),
        spell("Distort Value", 1, Illusion, false),
        spell("Shield", 1, Abjuration, false),
        spell("Mage Armor", 1, Abjuration, false),
        spell("Misty Step", 2, Conjuration, false),
        spell("Suggestion", 2, Enchantment, false),
        spell("Maximilian's Earthen Grasp", 2, Transmutation, true),
        spell("Lesser Restoration", 2, Abjuration, false),
        spell("Hypnotic Pattern", 3, Illusion, true),
        spell("Counterspell", 3, Abjuration, false),
        spell("Dispel Magic", 3, Abjuration, false),
    ];
    for sp in known {
        c.spells.spells.insert(sp.name.clone(), sp);
    }

    // Sorcery Points: 5, long-rest restore (engine default).
    c.meters.meters.insert(
        "sorcery_point".into(),
        Meter::new("sorcery_point".into(), 5, RestoreTime::default()),
    );

    // Features (text lives in the vault; names ride the sheet).
    for name in [
        "Restore Balance",
        "Metamagic: Heightened Spell",
        "Metamagic: Quickened Spell",
    ] {
        c.features.inner.push(blackletter_types::feature::Feature {
            name: name.into(),
            source: None,
            effects: Vec::new(),
        });
    }
    c.feats.push(blackletter_types::feat::Feat {
        name: "Keen Mind".into(),
        desc: "+1 Int (already baked into the 19). Always know which way is north, the time, and how long before sundown.".into(),
        feature: None,
        source: None,
    });

    // Inventory (gold is the note's 76 gp; 28 sp / 102 cp noted in lore).
    let items = [
        item("Ball bearings", 1),
        item("Dagger", 2),
        item("Spear", 1),
        item("Wand (spellcasting focus)", 1),
        item("Crowbar", 1),
        item("Hammer", 1),
        item("Piton", 1),
        item("Rations (1 day)", 1),
        item("Tinderbox", 1),
        item("Torch", 1),
        item("Waterskin", 1),
        item("Scarf (stolen from a Corash nobleman)", 1),
        item("Vial of the headmaster's blood", 1),
        item("Wand of smiles", 1),
        item("Jar of dirt", 1),
    ];
    let mut inv = Inventory::default();
    for it in items {
        inv.0.insert(it.name.clone(), it);
    }
    c.inventory = inv;
    c.gold = 76;

    c.lore = Lore {
        backstory: "Dreamed of being the greatest wizard; enrolled at Amberglade Academy at nine. Mother Anabelle vanished in an Unknown Magical Phenomenon. Graduated with his friend Eli, who pressed a vial of the headmaster's blood on him. Travels Endara under the banner of House Underwood (wealthy, not noble), feuding with Carden Jirah of House Jirah.".into(),
        personality_traits: String::new(),
        allies: "Eli (school friend), Yorhak (family retainer?)".into(),
        enemies: "Carden Jirah / House Jirah".into(),
        physical_traits: "Brown eyes, blond hair, pale skin; wealthy-ish clothing; \"disheveled disaster twink in slightly stylish clothing\" (Bethany). Found a number of scarves.".into(),
        ..Lore::default()
    };
    c.lore.personality_traits =
        "Carries 28 silver and 102 copper alongside the 76 gold. Thinks gnolls are gnomes.".into();

    c.sync_proficiency_bonus();
    c.sync_spell_slots();

    let path =
        std::path::Path::new("/home/nat/Documents/Campaign/characters/Aldric Underwood.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string_pretty(&c).unwrap()).unwrap();

    // Sanity: derived numbers vs the note.
    println!("wrote {}", path.display());
    println!("PB            {}", c.skills.proficiency_bonus);
    println!("AC            {}", c.armor_class_total());
    println!("max HP        {}", c.max_health_total());
    println!("save DC       {}", c.save_dc());
    println!("spell attack  {}", c.spell_bonus());
    println!(
        "slots L1..L3  {:?} {:?}",
        c.spell_slots(),
        c.meters.meters.keys().collect::<Vec<_>>()
    );
    for s in [Skill::Arcana, Skill::Persuasion, Skill::Perception] {
        println!("skill {s:<12} {}", c.skill_check(&s));
    }
    println!("languages     {}", c.languages);
}
