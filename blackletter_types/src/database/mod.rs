/// Structs to read 5e-database from dnd srd api.
#[cfg(feature = "serde")]
pub mod background;
#[cfg(feature = "serde")]
pub mod class;
pub mod condition;
#[cfg(feature = "serde")]
pub mod corpus;
pub mod feat;
pub mod feature;
#[cfg(feature = "serde")]
pub mod item;
#[cfg(feature = "serde")]
pub mod level;
pub mod options;
pub mod reference;
#[cfg(feature = "serde")]
pub mod subclass;

#[cfg(feature = "serde")]
pub mod generic;

#[cfg(feature = "serde")]
pub mod race;

#[cfg(feature = "serde")]
pub mod spell;
#[cfg(feature = "serde")]
pub mod traits;
