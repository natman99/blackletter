//! The 5e-database "option set" shape: `{ desc, choose, type, from }` —
//! how the SRD encodes every "choose X from Y" (class skill choices,
//! starting equipment alternatives, race languages). The creation wizard
//! (plan Task 5.1) drives its prompts from these.
//!
//! Kept a faithful raw mirror: `option_type`/`set_type` stay strings so a
//! corpus quirk never fails the load; the only interpretation here is
//! `candidates()`, which flattens the reference-bearing option types into
//! concrete picks. Free-form options (`option_type: "choice"`, e.g.
//! "choose a language") carry no `from` and surface through `desc`.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "serde")]
use super::item::EquipmentCategory;
#[cfg(feature = "serde")]
use super::reference::Reference;

/// One "choose N of these" prompt as the corpus encodes it.
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChoiceOption {
    pub desc: String,
    pub choose: i64,
    /// `"equipment"`, `"proficiencies"`, `"languages"`, ...
    #[serde(rename = "type")]
    pub kind: String,
    pub from: Option<OptionSet>,
}

/// The set a `ChoiceOption` draws from.
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OptionSet {
    #[serde(rename = "option_set_type")]
    pub set_type: String,
    pub options: Vec<OptionItem>,
    /// For `option_set_type: "equipment_category"`: the category whose
    /// equipment rows become the members (expanded at corpus load).
    pub equipment_category: Option<Reference>,
}

/// One member of an option set. Which fields are present depends on
/// `option_type`: `reference` carries `item`, `counted_reference` carries
/// `of` + `count`, `multiple` nests more `options`, and `choice` is
/// free-form text with no members.
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OptionItem {
    #[serde(rename = "option_type")]
    pub item_type: String,
    pub item: Option<Reference>,
    pub of: Option<Reference>,
    pub count: Option<i64>,
    pub options: Vec<OptionItem>,
    pub desc: String,
    /// For `option_type: "choice"`: a nested option set (e.g. "an arcane
    /// focus" inside the wizard's starting equipment).
    pub choice: Option<ChoiceOption>,
}

/// A concrete pick flattened out of an option set: what the sheet would
/// actually grant if the user chooses it.
#[cfg(feature = "serde")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub index: String,
    pub name: String,
    pub quantity: i64,
}

#[cfg(feature = "serde")]
impl ChoiceOption {
    /// Flatten the reference-bearing members into concrete candidates,
    /// recursing through `multiple` groups. Empty for free-form options.
    #[must_use]
    pub fn candidates(&self) -> Vec<Candidate> {
        let mut out = Vec::new();
        if let Some(set) = &self.from {
            for item in &set.options {
                item.flatten(&mut out);
            }
        }
        out
    }
}

#[cfg(feature = "serde")]
impl OptionItem {
    fn flatten(&self, out: &mut Vec<Candidate>) {
        match self.item_type.as_str() {
            "reference" => {
                if let Some(r) = &self.item {
                    out.push(Candidate {
                        index: r.index.clone(),
                        name: r.name.clone(),
                        quantity: 1,
                    });
                }
            }
            "counted_reference" => {
                if let Some(r) = &self.of {
                    out.push(Candidate {
                        index: r.index.clone(),
                        name: r.name.clone(),
                        quantity: self.count.unwrap_or(1).max(1),
                    });
                }
            }
            "multiple" => {
                for inner in &self.options {
                    inner.flatten(out);
                }
            }
            "choice" => {
                // Nested set: flatten its members up (the nested choose
                // count is subsumed by the parent's).
                if let Some(c) = &self.choice
                    && let Some(set) = &c.from
                {
                    for inner in &set.options {
                        inner.flatten(out);
                    }
                }
            }
            // Any future type: desc-only.
            _ => {}
        }
    }
}

#[cfg(feature = "serde")]
impl ChoiceOption {
    /// Replace `equipment_category` sets with concrete reference members
    /// drawn from the corpus's equipment table, recursively. Called once
    /// at corpus load so `candidates()` stays a pure function.
    pub fn expand_categories(&mut self, categories: &[EquipmentCategory]) {
        if let Some(set) = &mut self.from {
            expand_set(set, categories);
        }
    }
}

#[cfg(feature = "serde")]
fn expand_set(set: &mut OptionSet, categories: &[EquipmentCategory]) {
    if set.set_type == "equipment_category"
        && let Some(cat) = &set.equipment_category
        && let Some(row) = categories.iter().find(|e| e.index == cat.index)
    {
        set.options = row
            .equipment
            .iter()
            .map(|r| OptionItem {
                item_type: "reference".into(),
                item: Some(Reference::new(&r.index, &r.name)),
                ..Default::default()
            })
            .collect();
        set.set_type = "options_array".into();
    }
    for item in &mut set.options {
        if let Some(c) = &mut item.choice
            && let Some(inner) = &mut c.from
        {
            expand_set(inner, categories);
        }
    }
}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::*;

    #[cfg(feature = "serde")]
    fn classes() -> Vec<super::super::class::DatabaseClass> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../srd/2014/en/5e-SRD-Classes.json"
        );
        let text = std::fs::read_to_string(path).expect("vendored SRD classes");
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn class_choice_fields_parse_from_the_corpus() {
        let all = classes();
        let wizard = all.iter().find(|c| c.index == "wizard").unwrap();
        assert_eq!(wizard.proficiency_choices.len(), 1);
        assert_eq!(wizard.proficiency_choices[0].choose, 2);
        assert_eq!(wizard.proficiency_choices[0].kind, "proficiencies");
        assert_eq!(wizard.starting_equipment.len(), 1);
        assert_eq!(wizard.starting_equipment[0].equipment.index, "spellbook");
        assert!(!wizard.starting_equipment_options.is_empty());

        // Every class row must deserialize without error (already proven
        // by the load); spot-check a weapon-vs-weapon alternative.
        let fighter = all.iter().find(|c| c.index == "fighter").unwrap();
        assert!(!fighter.starting_equipment_options.is_empty());
        assert!(!fighter.proficiency_choices.is_empty());
    }

    #[test]
    fn candidates_flatten_reference_types() {
        let mut all = classes();
        // Mirror what Corpus::load does: expand equipment_category sets
        // into concrete members before flattening.
        let cat_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../srd/2014/en/5e-SRD-Equipment-Categories.json"
        );
        let cats: Vec<super::super::item::EquipmentCategory> =
            serde_json::from_str(&std::fs::read_to_string(cat_path).unwrap()).unwrap();
        for class in &mut all {
            for opt in &mut class.starting_equipment_options {
                opt.expand_categories(&cats);
            }
        }
        let wizard = all.iter().find(|c| c.index == "wizard").unwrap();
        // "(a) a quarterstaff or (b) a dagger"
        let opt = &wizard.starting_equipment_options[0];
        let cands = opt.candidates();
        assert_eq!(cands.len(), 2);
        assert_eq!(cands[0].index, "quarterstaff");
        assert_eq!(cands[0].quantity, 1);
        assert_eq!(cands[1].index, "dagger");

        // "(a) a component pouch or (b) an arcane focus": a
        // counted_reference plus a nested equipment_category choice —
        // expanded at load, flattened by candidates().
        let opt = &wizard.starting_equipment_options[1];
        let cands = opt.candidates();
        assert_eq!(cands[0].index, "component-pouch");
        assert!(
            cands.iter().any(|c| c.index == "crystal"),
            "arcane foci expanded: {cands:?}"
        );

        // Skill choices flatten to `skill-*` references.
        let skills = wizard.proficiency_choices[0].candidates();
        let indexes: Vec<&str> = skills.iter().map(|c| c.index.as_str()).collect();
        assert_eq!(
            indexes,
            vec![
                "skill-arcana",
                "skill-history",
                "skill-insight",
                "skill-investigation",
                "skill-medicine",
                "skill-religion"
            ]
        );
    }
}
