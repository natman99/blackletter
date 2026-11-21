//! Headless dungeon-map model: rooms, doors, floors, pathfinding, saves.
//! Ported from the standalone `dungeon-tracker` app (gui/ left behind).

pub mod dungeon;
pub mod model;
pub mod save;

pub use model::{
    Connection, Direction, Door, DoorId, Dungeon, DungeonError, EnemyState, Floor, FloorId,
    LootState, Room, RoomId, TrapState, VerticalKind,
};
