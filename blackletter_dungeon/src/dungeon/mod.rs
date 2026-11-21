pub mod invariants;
pub mod path;

use glam::IVec2;

use crate::model::{
    Connection, Direction, Door, DoorId, Dungeon, DungeonError, Floor, FloorId, Room, RoomId,
    VerticalKind,
};

impl Dungeon {
    /// Create a fresh dungeon: one floor (`D1`) with a single entrance room at
    /// the origin. The party starts at the entrance.
    pub fn new(name: impl Into<String>) -> Self {
        let mut dungeon = Self {
            name: name.into(),
            entrance: RoomId(0),
            floors: Vec::new(),
            rooms: Vec::new(),
            doors: Vec::new(),
            party_position: None,
        };
        let floor = dungeon.add_floor("D1");
        let entrance = dungeon.create_room_on_floor(floor, IVec2::ZERO, None);
        dungeon.entrance = entrance;
        dungeon.party_position = Some(entrance);
        if let Some(r) = dungeon.room_mut(entrance) {
            r.name = Some("Entrance".into());
        }
        dungeon
    }

    // ---- lookups -----------------------------------------------------------

    pub fn room(&self, id: RoomId) -> Option<&Room> {
        self.rooms.iter().find(|r| r.id == id)
    }

    pub fn room_mut(&mut self, id: RoomId) -> Option<&mut Room> {
        self.rooms.iter_mut().find(|r| r.id == id)
    }

    pub fn floor(&self, id: FloorId) -> Option<&Floor> {
        self.floors.iter().find(|f| f.id == id)
    }

    /// The room occupying `pos` on `floor`, if any.
    pub fn room_at(&self, floor: FloorId, pos: IVec2) -> Option<RoomId> {
        self.rooms
            .iter()
            .find(|r| r.floor == floor && r.position == pos)
            .map(|r| r.id)
    }

    // ---- floors ------------------------------------------------------------

    /// Add a new (empty) floor at the bottom.
    pub fn add_floor(&mut self, name: impl Into<String>) -> FloorId {
        let id = self.next_floor_id();
        self.floors.push(Floor {
            id,
            name: name.into(),
        });
        id
    }

    pub fn rename_floor(
        &mut self,
        id: FloorId,
        name: impl Into<String>,
    ) -> Result<(), DungeonError> {
        match self.floors.iter_mut().find(|f| f.id == id) {
            Some(f) => {
                f.name = name.into();
                Ok(())
            }
            None => Err(DungeonError::FloorNotFound(id)),
        }
    }

    // ---- room creation -----------------------------------------------------

    fn create_room_on_floor(
        &mut self,
        floor: FloorId,
        pos: IVec2,
        parent: Option<RoomId>,
    ) -> RoomId {
        let id = self.next_room_id();
        self.rooms.push(Room::new(id, floor, pos, parent));
        id
    }

    /// Add a door from `from` in grid direction `dir`. Creates the target room
    /// (parent = `from`) if the adjacent cell is empty, otherwise links to the
    /// existing room. Returns the target room.
    pub fn add_horizontal_door(
        &mut self,
        from: RoomId,
        dir: Direction,
    ) -> Result<RoomId, DungeonError> {
        let (floor, pos) = {
            let from_room = self.room(from).ok_or(DungeonError::RoomNotFound(from))?;
            (from_room.floor, from_room.position + dir.offset())
        };

        let target = match self.room_at(floor, pos) {
            Some(existing) => existing,
            None => self.create_room_on_floor(floor, pos, Some(from)),
        };

        self.link_doors(from, target, Connection::Horizontal(dir));
        Ok(target)
    }

    /// Add a vertical door from `from` to the floor above/below. Creates the
    /// target room (directly above/below, same cell) if that cell is empty,
    /// otherwise links to the existing room. Returns the target room.
    pub fn add_vertical_door(
        &mut self,
        from: RoomId,
        up: bool,
        kind: VerticalKind,
    ) -> Result<RoomId, DungeonError> {
        let (from_pos, from_floor_idx) = {
            let from_room = self.room(from).ok_or(DungeonError::RoomNotFound(from))?;
            let from_floor_idx = self
                .floors
                .iter()
                .position(|f| f.id == from_room.floor)
                .ok_or(DungeonError::FloorNotFound(from_room.floor))?;
            (from_room.position, from_floor_idx)
        };

        let target_floor_idx = if up {
            if from_floor_idx == 0 {
                return Err(DungeonError::NoFloorAbove);
            }
            from_floor_idx - 1
        } else {
            from_floor_idx + 1
        };

        // Create the floor below if we're descending into it for the first time.
        if target_floor_idx >= self.floors.len() {
            let name = format!("D{}", self.floors.len() + 1);
            self.add_floor(name);
        }

        let target_floor = self.floors[target_floor_idx].id;
        let target = match self.room_at(target_floor, from_pos) {
            Some(existing) => existing,
            None => self.create_room_on_floor(target_floor, from_pos, Some(from)),
        };

        self.link_doors(from, target, Connection::Vertical { kind });
        Ok(target)
    }

    /// Ensure a door exists between `from` and `target` for `conn`. Doors are
    /// stored once per connection (`from`/`target` mark the two ends), so
    /// editing one end edits both.
    fn link_doors(&mut self, from: RoomId, target: RoomId, conn: Connection) {
        let already = self.doors.iter().any(|d| {
            (d.from == from && d.target == target) || (d.from == target && d.target == from)
        });
        if already {
            return;
        }
        self.doors.push(Door {
            id: self.next_door_id(),
            from,
            target,
            connection: conn,
            name: None,
            notes: String::new(),
            locked: false,
            trapped: false,
            secret: false,
        });
    }

    // ---- deletion ----------------------------------------------------------

    /// Delete a room and all its doors. Children in the parent tree are
    /// re-parented to the deleted room's parent, preserving the invariant that
    /// every room reaches the entrance.
    pub fn delete_room(&mut self, id: RoomId) -> Result<(), DungeonError> {
        if id == self.entrance {
            return Err(DungeonError::EntranceProtected);
        }
        let parent = match self.room(id) {
            Some(r) => r.parent,
            None => return Err(DungeonError::RoomNotFound(id)),
        };

        // Re-parent orphans so the parent tree stays connected to the entrance.
        for room in self.rooms.iter_mut() {
            if room.parent == Some(id) {
                room.parent = parent;
            }
        }

        // Remove the room's doors.
        self.doors.retain(|d| d.from != id && d.target != id);

        // Swap-remove (ids are stable, so no re-indexing of references needed).
        if let Some(idx) = self.rooms.iter().position(|r| r.id == id) {
            self.rooms.swap_remove(idx);
        }

        if self.party_position == Some(id) {
            self.party_position = None;
        }

        Ok(())
    }

    /// Delete a door (both ends lose the connection).
    pub fn delete_door(&mut self, id: DoorId) -> Result<(), DungeonError> {
        if !self.doors.iter().any(|d| d.id == id) {
            return Err(DungeonError::DoorNotFound(id));
        }
        self.doors.retain(|d| d.id != id);
        Ok(())
    }

    /// Delete a room if the dungeon stays connected to the entrance, then drop
    /// any floors left empty. Rejected (state unchanged) when the deletion
    /// would orphan rooms or floors.
    pub fn try_delete_room(&mut self, id: RoomId) -> Result<(), DungeonError> {
        if id == self.entrance {
            return Err(DungeonError::EntranceProtected);
        }
        if self.room(id).is_none() {
            return Err(DungeonError::RoomNotFound(id));
        }
        let mut next = self.clone();
        next.delete_room(id).expect("room checked above");
        if !path::is_connected(&next) {
            return Err(DungeonError::WouldDisconnect);
        }
        next.remove_empty_floors();
        *self = next;
        Ok(())
    }

    /// Delete a door if the dungeon stays connected to the entrance, then drop
    /// any floors left empty. Rejected (state unchanged) when the deletion
    /// would orphan rooms or floors.
    pub fn try_delete_door(&mut self, id: DoorId) -> Result<(), DungeonError> {
        if !self.doors.iter().any(|d| d.id == id) {
            return Err(DungeonError::DoorNotFound(id));
        }
        let mut next = self.clone();
        next.delete_door(id).expect("door checked above");
        if !path::is_connected(&next) {
            return Err(DungeonError::WouldDisconnect);
        }
        next.remove_empty_floors();
        *self = next;
        Ok(())
    }

    /// Remove floors that have no rooms. (D1 can never become empty: the
    /// entrance lives there and is protected.)
    fn remove_empty_floors(&mut self) {
        self.floors
            .retain(|f| self.rooms.iter().any(|r| r.floor == f.id));
    }

    // ---- party -------------------------------------------------------------

    pub fn set_party_position(&mut self, pos: Option<RoomId>) {
        self.party_position = pos;
    }

    // ---- id allocation -----------------------------------------------------

    fn next_room_id(&self) -> RoomId {
        RoomId(
            self.rooms
                .iter()
                .map(|r| r.id.0)
                .max()
                .map(|m| m + 1)
                .unwrap_or(0),
        )
    }

    fn next_door_id(&self) -> DoorId {
        DoorId(
            self.doors
                .iter()
                .map(|d| d.id.0)
                .max()
                .map(|m| m + 1)
                .unwrap_or(0),
        )
    }

    fn next_floor_id(&self) -> FloorId {
        FloorId(
            self.floors
                .iter()
                .map(|f| f.id.0)
                .max()
                .map(|m| m + 1)
                .unwrap_or(0),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::dungeon::invariants::validate;
    use crate::model::{Direction, Dungeon, DungeonError, FloorId, RoomId, VerticalKind};
    use glam::IVec2;

    #[test]
    fn new_dungeon_is_valid() {
        let d = Dungeon::new("test");
        assert_eq!(d.rooms.len(), 1);
        assert_eq!(d.floors.len(), 1);
        assert_eq!(d.doors.len(), 0);
        assert!(d.room(d.entrance).is_some());
        assert!(d.room(d.entrance).unwrap().parent.is_none());
        assert_eq!(d.party_position, Some(d.entrance));
        assert!(validate(&d).is_ok());
    }

    #[test]
    fn add_horizontal_door_creates_room_with_parent() {
        let mut d = Dungeon::new("t");
        let target = d.add_horizontal_door(d.entrance, Direction::E).unwrap();
        let room = d.room(target).unwrap();
        assert_eq!(room.position, IVec2::new(1, 0));
        assert_eq!(room.parent, Some(d.entrance));
        assert_eq!(d.rooms.len(), 2);
        assert_eq!(d.doors.len(), 1);
        assert!(validate(&d).is_ok());
    }

    #[test]
    fn add_horizontal_door_to_occupied_links_existing() {
        let mut d = Dungeon::new("t");
        let a = d.add_horizontal_door(d.entrance, Direction::E).unwrap();
        let back = d.add_horizontal_door(a, Direction::W).unwrap();
        assert_eq!(back, d.entrance);
        assert_eq!(d.rooms.len(), 2);
        assert_eq!(d.doors.len(), 1); // no duplicate doors
        assert!(validate(&d).is_ok());
    }

    #[test]
    fn all_eight_directions_place_adjacent_rooms() {
        let mut d = Dungeon::new("t");
        let origin = d.room(d.entrance).unwrap().position;
        for dir in Direction::ALL {
            let r = d.add_horizontal_door(d.entrance, dir).unwrap();
            assert_eq!(d.room(r).unwrap().position, origin + dir.offset());
        }
        assert!(validate(&d).is_ok());
    }

    #[test]
    fn vertical_door_down_creates_floor_and_room() {
        let mut d = Dungeon::new("t");
        let target = d
            .add_vertical_door(d.entrance, false, VerticalKind::Stairs)
            .unwrap();
        let room = d.room(target).unwrap();
        assert_eq!(room.floor, FloorId(1));
        assert_eq!(room.position, IVec2::ZERO);
        assert_eq!(room.parent, Some(d.entrance));
        assert_eq!(d.floors.len(), 2);
        assert!(validate(&d).is_ok());
    }

    #[test]
    fn vertical_door_up_from_top_is_rejected() {
        let mut d = Dungeon::new("t");
        assert_eq!(
            d.add_vertical_door(d.entrance, true, VerticalKind::Stairs),
            Err(DungeonError::NoFloorAbove)
        );
    }

    #[test]
    fn vertical_door_up_after_down_links_back() {
        let mut d = Dungeon::new("t");
        let down = d
            .add_vertical_door(d.entrance, false, VerticalKind::Stairs)
            .unwrap();
        let up = d
            .add_vertical_door(down, true, VerticalKind::Stairs)
            .unwrap();
        assert_eq!(up, d.entrance);
        assert_eq!(d.floors.len(), 2);
        assert!(validate(&d).is_ok());
    }

    #[test]
    fn delete_room_reparents_children() {
        let mut d = Dungeon::new("t");
        let a = d.add_horizontal_door(d.entrance, Direction::E).unwrap();
        let b = d.add_horizontal_door(a, Direction::E).unwrap();
        assert_eq!(d.room(b).unwrap().parent, Some(a));
        d.delete_room(a).unwrap();
        assert!(d.room(a).is_none());
        assert_eq!(d.room(b).unwrap().parent, Some(d.entrance));
        assert!(validate(&d).is_ok());
    }

    #[test]
    fn delete_room_removes_doors() {
        let mut d = Dungeon::new("t");
        let a = d.add_horizontal_door(d.entrance, Direction::E).unwrap();
        assert_eq!(d.doors.len(), 1);
        d.delete_room(a).unwrap();
        assert_eq!(d.doors.len(), 0);
        assert!(validate(&d).is_ok());
    }

    #[test]
    fn delete_entrance_is_protected() {
        let mut d = Dungeon::new("t");
        assert_eq!(
            d.delete_room(d.entrance),
            Err(DungeonError::EntranceProtected)
        );
    }

    #[test]
    fn delete_room_clears_party_position() {
        let mut d = Dungeon::new("t");
        let a = d.add_horizontal_door(d.entrance, Direction::E).unwrap();
        d.set_party_position(Some(a));
        d.delete_room(a).unwrap();
        assert_eq!(d.party_position, None);
    }

    #[test]
    fn unknown_room_errors() {
        let mut d = Dungeon::new("t");
        let ghost = RoomId(999);
        assert_eq!(
            d.add_horizontal_door(ghost, Direction::E),
            Err(DungeonError::RoomNotFound(ghost))
        );
        assert_eq!(d.delete_room(ghost), Err(DungeonError::RoomNotFound(ghost)));
    }

    /// D1(entrance) - D2(a) - D3(b), one room per floor, chained by stairs.
    fn three_floors() -> (Dungeon, RoomId, RoomId) {
        let mut d = Dungeon::new("f");
        let a = d
            .add_vertical_door(d.entrance, false, VerticalKind::Stairs)
            .unwrap();
        let b = d.add_vertical_door(a, false, VerticalKind::Stairs).unwrap();
        (d, a, b)
    }

    fn door_between(d: &Dungeon, a: RoomId, b: RoomId) -> crate::model::DoorId {
        d.doors
            .iter()
            .find(|d| (d.from == a && d.target == b) || (d.from == b && d.target == a))
            .unwrap()
            .id
    }

    #[test]
    fn delete_door_removes_door() {
        let mut d = Dungeon::new("t");
        let a = d.add_horizontal_door(d.entrance, Direction::E).unwrap();
        let door = door_between(&d, d.entrance, a);
        assert_eq!(d.doors.len(), 1);
        d.delete_door(door).unwrap();
        assert_eq!(d.doors.len(), 0);
        // Both rooms remain (they're just no longer linked).
        assert_eq!(d.rooms.len(), 2);
    }

    #[test]
    fn try_delete_door_removes_pair_when_not_a_bridge() {
        // A 2x2 loop: entrance - b - c - e - entrance.
        let mut d = Dungeon::new("t");
        let b = d.add_horizontal_door(d.entrance, Direction::E).unwrap();
        let c = d.add_horizontal_door(b, Direction::S).unwrap();
        let e = d.add_horizontal_door(c, Direction::W).unwrap();
        d.add_horizontal_door(e, Direction::N).unwrap();
        assert_eq!(d.rooms.len(), 4);
        assert_eq!(d.doors.len(), 4);
        let door = door_between(&d, d.entrance, b);
        d.try_delete_door(door).unwrap();
        assert_eq!(d.doors.len(), 3);
        assert_eq!(d.rooms.len(), 4);
        assert!(validate(&d).is_ok());
    }

    #[test]
    fn try_delete_room_orphaning_a_floor_is_rejected() {
        let (mut d, a, _b) = three_floors();
        assert_eq!(d.try_delete_room(a), Err(DungeonError::WouldDisconnect));
        // State untouched.
        assert!(d.room(a).is_some());
        assert_eq!(d.rooms.len(), 3);
        assert_eq!(d.floors.len(), 3);
        assert!(validate(&d).is_ok());
    }

    #[test]
    fn try_delete_door_orphaning_a_floor_is_rejected() {
        let (mut d, a, _b) = three_floors();
        let door = door_between(&d, d.entrance, a);
        assert_eq!(d.try_delete_door(door), Err(DungeonError::WouldDisconnect));
        assert_eq!(d.doors.len(), 2); // entrance↔a and a↔b intact
        assert_eq!(d.floors.len(), 3);
    }

    #[test]
    fn try_delete_room_last_room_on_floor_removes_floor() {
        let (mut d, a, b) = three_floors();
        // Delete b first (floor 3 becomes empty and is removed).
        d.try_delete_room(b).unwrap();
        assert_eq!(d.floors.len(), 2);
        // Now a is deletable too.
        d.try_delete_room(a).unwrap();
        assert_eq!(d.floors.len(), 1);
        assert_eq!(d.rooms.len(), 1);
        assert!(validate(&d).is_ok());
    }

    #[test]
    fn try_delete_door_last_link_to_nonempty_floor_is_rejected() {
        let (mut d, a, b) = three_floors();
        // Remove floor 3 first, so the a↔entrance stairs are floor 2's only link.
        d.try_delete_room(b).unwrap();
        let door = door_between(&d, d.entrance, a);
        // Room a still lives on floor 2, so the stairs can't be removed
        // (floors only auto-remove when they become empty).
        assert_eq!(d.try_delete_door(door), Err(DungeonError::WouldDisconnect));
        assert_eq!(d.floors.len(), 2);
        assert_eq!(d.rooms.len(), 2);
    }

    #[test]
    fn try_delete_dead_end_room_with_floor_below_is_rejected() {
        // entrance - E on floor 1, plus E - D2(a) stairs: E is the only
        // bridge to floor 2, so deleting it must be rejected.
        let mut d = Dungeon::new("t");
        let e = d.add_horizontal_door(d.entrance, Direction::E).unwrap();
        let a = d.add_vertical_door(e, false, VerticalKind::Stairs).unwrap();
        assert_eq!(d.try_delete_room(e), Err(DungeonError::WouldDisconnect));
        assert!(d.room(e).is_some());
        // The door from a back up to e is likewise a bridge.
        let door = door_between(&d, e, a);
        assert_eq!(d.try_delete_door(door), Err(DungeonError::WouldDisconnect));
    }
}
