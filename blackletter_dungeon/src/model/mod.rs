pub mod direction;
pub mod states;

use glam::IVec2;

pub use direction::{Connection, Direction, VerticalKind};
pub use states::{EnemyState, LootState, TrapState};

/// Stable identifier for a room. Ids are kept stable across mutations so they
/// can be safely referenced (deletion does not renumber other rooms).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct RoomId(pub u32);

/// Stable identifier for a door.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct DoorId(pub u32);

/// Stable identifier for a floor.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct FloorId(pub u32);

impl std::fmt::Display for RoomId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Display for DoorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Display for FloorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A single level of the dungeon. Floor 0 is the topmost (entrance) level;
/// higher floor numbers are deeper.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Floor {
    pub id: FloorId,
    pub name: String,
}

/// A room on the grid.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Room {
    pub id: RoomId,
    /// Which floor this room lives on.
    pub floor: FloorId,
    /// Grid cell on this floor.
    pub position: IVec2,
    /// Spanning-tree parent. Following `parent` pointers always reaches the
    /// entrance. Set when the room is first discovered, never changed after.
    pub parent: Option<RoomId>,
    pub name: Option<String>,
    pub notes: String,
    /// An additional exit. The entrance itself is tracked separately.
    pub is_exit: bool,
    /// The party has searched this room for loot/secrets.
    pub searched: bool,
    /// Trap state (none / active / disarmed).
    pub trap: TrapState,
    pub enemies: EnemyState,
    pub loot: LootState,
    /// A secret room.
    pub secret: bool,
}

impl Room {
    /// Create a room with default (unexplored) state.
    pub fn new(id: RoomId, floor: FloorId, position: IVec2, parent: Option<RoomId>) -> Self {
        Self {
            id,
            floor,
            position,
            parent,
            name: None,
            notes: String::new(),
            is_exit: false,
            searched: false,
            trap: TrapState::default(),
            enemies: EnemyState::default(),
            loot: LootState::default(),
            secret: false,
        }
    }
}

/// A connection between two rooms. Doors are stored in pairs (one per side).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(from = "DoorShadow")]
pub struct Door {
    pub id: DoorId,
    pub from: RoomId,
    pub target: RoomId,
    pub connection: Connection,
    pub name: Option<String>,
    pub notes: String,
    /// `None` = unlocked. The SRD has exactly ONE lock DC — DC 15 Dexterity
    /// with thieves' tools (`Wiki/04 equipment.md:316`); there is no SRD
    /// easy/medium/hard table (that's DMG), so the presets below are the SRD
    /// default and one hand-picked harder value, and the DC is free-editable.
    pub lock: Option<Lock>,
    pub trapped: bool,
    /// A secret / hidden door.
    pub secret: bool,
    /// Currently open (serde default: closed).
    pub open: bool,
}

/// A lock's pick DC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Lock {
    pub dc: i64,
}

impl Lock {
    /// The SRD lock: DC 15 Dex (thieves' tools), `Wiki/04 equipment.md:316`.
    pub const SRD: Lock = Lock { dc: 15 };
    /// A hand-picked "better lock" for the `L` cycle.
    pub const HARD: Lock = Lock { dc: 25 };
}

/// Deserialization shadow so pre-upgrade saves (`locked: bool`, no `open`)
/// keep loading: `locked: true` migrates to the SRD lock.
#[derive(serde::Deserialize)]
struct DoorShadow {
    id: DoorId,
    from: RoomId,
    target: RoomId,
    connection: Connection,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    lock: Option<Lock>,
    #[serde(default)]
    locked: Option<bool>,
    #[serde(default)]
    trapped: bool,
    #[serde(default)]
    secret: bool,
    #[serde(default)]
    open: bool,
}

impl From<DoorShadow> for Door {
    fn from(s: DoorShadow) -> Self {
        Self {
            id: s.id,
            from: s.from,
            target: s.target,
            connection: s.connection,
            name: s.name,
            notes: s.notes,
            lock: s.lock.or(s.locked.unwrap_or(false).then_some(Lock::SRD)),
            trapped: s.trapped,
            secret: s.secret,
            open: s.open,
        }
    }
}

/// The whole dungeon.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Dungeon {
    /// Display name, also used as the save filename.
    pub name: String,
    /// The entrance room — the root of the parent tree.
    pub entrance: RoomId,
    pub floors: Vec<Floor>,
    pub rooms: Vec<Room>,
    pub doors: Vec<Door>,
    /// Where the party is right now (`None` = off the map).
    pub party_position: Option<RoomId>,
}

/// Errors from mutating operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DungeonError {
    RoomNotFound(RoomId),
    DoorNotFound(DoorId),
    FloorNotFound(FloorId),
    /// The entrance room cannot be deleted.
    EntranceProtected,
    /// Tried to go above the topmost floor.
    NoFloorAbove,
    /// The deletion would leave rooms/floors unreachable from the entrance.
    WouldDisconnect,
}

impl std::fmt::Display for DungeonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DungeonError::RoomNotFound(id) => write!(f, "room {id} not found"),
            DungeonError::DoorNotFound(id) => write!(f, "door {id} not found"),
            DungeonError::FloorNotFound(id) => write!(f, "floor {id} not found"),
            DungeonError::EntranceProtected => write!(f, "the entrance cannot be deleted"),
            DungeonError::NoFloorAbove => write!(f, "there is no floor above the top floor"),
            DungeonError::WouldDisconnect => {
                write!(f, "deletion would leave part of the dungeon unreachable")
            }
        }
    }
}

impl std::error::Error for DungeonError {}
