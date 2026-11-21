use std::collections::HashSet;

use glam::IVec2;

use crate::model::{Connection, Dungeon, RoomId};

/// A single invariant violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvariantViolation {
    EntranceMissing,
    EntranceHasParent,
    /// A room's parent chain does not reach the entrance (broken, cycled, or a
    /// second root).
    RoomNotReachable {
        room: RoomId,
    },
    DoorDanglingFrom {
        door: u32,
        from: RoomId,
    },
    DoorDanglingTarget {
        door: u32,
        target: RoomId,
    },
    HorizontalDoorNotAdjacent {
        door: u32,
    },
    HorizontalDoorCrossesFloors {
        door: u32,
    },
    DuplicateCell {
        floor: u32,
        position: IVec2,
    },
    PartyPositionDangling,
}

/// Check all invariants. Returns `Ok(())` if the dungeon is consistent.
pub fn validate(d: &Dungeon) -> Result<(), Vec<InvariantViolation>> {
    let mut errs = Vec::new();

    match d.room(d.entrance) {
        None => errs.push(InvariantViolation::EntranceMissing),
        Some(r) => {
            if r.parent.is_some() {
                errs.push(InvariantViolation::EntranceHasParent);
            }
        }
    }

    for room in &d.rooms {
        if !reaches_entrance(d, room.id) {
            errs.push(InvariantViolation::RoomNotReachable { room: room.id });
        }
    }

    for door in &d.doors {
        if d.room(door.from).is_none() {
            errs.push(InvariantViolation::DoorDanglingFrom {
                door: door.id.0,
                from: door.from,
            });
        }
        if d.room(door.target).is_none() {
            errs.push(InvariantViolation::DoorDanglingTarget {
                door: door.id.0,
                target: door.target,
            });
        }
        if let Connection::Horizontal(dir) = door.connection
            && let (Some(a), Some(b)) = (d.room(door.from), d.room(door.target))
        {
            if a.floor != b.floor {
                errs.push(InvariantViolation::HorizontalDoorCrossesFloors { door: door.id.0 });
            }
            if a.position + dir.offset() != b.position {
                errs.push(InvariantViolation::HorizontalDoorNotAdjacent { door: door.id.0 });
            }
        }
    }

    let mut seen: HashSet<(u32, i32, i32)> = HashSet::new();
    for room in &d.rooms {
        let key = (room.floor.0, room.position.x, room.position.y);
        if !seen.insert(key) {
            errs.push(InvariantViolation::DuplicateCell {
                floor: room.floor.0,
                position: room.position,
            });
        }
    }

    if let Some(p) = d.party_position
        && d.room(p).is_none()
    {
        errs.push(InvariantViolation::PartyPositionDangling);
    }

    if errs.is_empty() { Ok(()) } else { Err(errs) }
}

/// Follow `parent` pointers from `start`. True iff the chain reaches the
/// entrance without a cycle or dangling pointer.
fn reaches_entrance(d: &Dungeon, start: RoomId) -> bool {
    let mut visited = HashSet::new();
    let mut current = start;
    loop {
        if !visited.insert(current) {
            return false; // cycle
        }
        let room = match d.room(current) {
            Some(r) => r,
            None => return false, // dangling
        };
        if current == d.entrance {
            return room.parent.is_none(); // must be the root
        }
        current = match room.parent {
            Some(p) => p,
            None => return false, // a second root
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Direction, Dungeon};

    #[test]
    fn fresh_dungeon_is_valid() {
        let d = Dungeon::new("t");
        assert!(validate(&d).is_ok());
    }

    #[test]
    fn broken_parent_is_flagged() {
        let mut d = Dungeon::new("t");
        let a = d.add_horizontal_door(d.entrance, Direction::E).unwrap();
        d.room_mut(a).unwrap().parent = Some(crate::model::RoomId(999));
        let errs = validate(&d).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, InvariantViolation::RoomNotReachable { .. }))
        );
    }

    #[test]
    fn duplicate_cell_and_bad_door_are_flagged() {
        let mut d = Dungeon::new("t");
        let a = d.add_horizontal_door(d.entrance, Direction::E).unwrap();
        // Force a duplicate cell on the same floor (which also breaks the door).
        d.room_mut(a).unwrap().position = IVec2::ZERO;
        let errs = validate(&d).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, InvariantViolation::DuplicateCell { .. }))
        );
        assert!(
            errs.iter()
                .any(|e| matches!(e, InvariantViolation::HorizontalDoorNotAdjacent { .. }))
        );
    }
}
