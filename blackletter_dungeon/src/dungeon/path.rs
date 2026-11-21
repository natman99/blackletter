use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::{Dungeon, EnemyState, RoomId, TrapState};

/// Follow `parent` pointers from `start` back to the entrance.
///
/// Returns the room ids from `start` (first) to the entrance (last). Returns
/// `None` if the chain is broken or cycles.
pub fn path_to_entrance(d: &Dungeon, start: RoomId) -> Option<Vec<RoomId>> {
    let mut path = Vec::new();
    let mut visited = HashSet::new();
    let mut current = start;
    loop {
        if !visited.insert(current) {
            return None; // cycle
        }
        path.push(current);
        if current == d.entrance {
            return Some(path);
        }
        current = {
            let r = d.room(current)?;
            r.parent?
        };
    }
}

/// True if every room is reachable from the entrance through the door graph
/// (horizontal and vertical doors alike).
pub fn is_connected(d: &Dungeon) -> bool {
    let mut visited: HashSet<RoomId> = HashSet::new();
    let mut stack = vec![d.entrance];
    while let Some(cur) = stack.pop() {
        if !visited.insert(cur) {
            continue;
        }
        for door in &d.doors {
            // Doors are undirected: either end can be the current room.
            let neighbor = if door.from == cur {
                Some(door.target)
            } else if door.target == cur {
                Some(door.from)
            } else {
                None
            };
            if let Some(n) = neighbor
                && !visited.contains(&n)
            {
                stack.push(n);
            }
        }
    }
    visited.len() == d.rooms.len()
}

/// Shortest path (fewest doors) between two rooms, walking the door graph.
/// Horizontal and vertical doors are both edges, so this works across floors.
///
/// Returns the room ids from `from` (first) to `to` (last), or `None` if there
/// is no path.
pub fn find_path(d: &Dungeon, from: RoomId, to: RoomId) -> Option<Vec<RoomId>> {
    if from == to {
        return Some(vec![from]);
    }

    let mut adj: HashMap<RoomId, Vec<RoomId>> = HashMap::new();
    for door in &d.doors {
        adj.entry(door.from).or_default().push(door.target);
        adj.entry(door.target).or_default().push(door.from);
    }

    let mut visited: HashSet<RoomId> = HashSet::new();
    let mut parent: HashMap<RoomId, RoomId> = HashMap::new();
    let mut queue: VecDeque<RoomId> = VecDeque::new();
    queue.push_back(from);
    visited.insert(from);

    while let Some(cur) = queue.pop_front() {
        if cur == to {
            break;
        }
        if let Some(neighbors) = adj.get(&cur) {
            for &n in neighbors {
                if !visited.contains(&n) {
                    visited.insert(n);
                    parent.insert(n, cur);
                    queue.push_back(n);
                }
            }
        }
    }

    if !visited.contains(&to) {
        return None;
    }

    let mut path = vec![to];
    let mut cur = to;
    while cur != from {
        cur = parent[&cur];
        path.push(cur);
    }
    path.reverse();
    Some(path)
}

/// A room is dangerous if it has active enemies or an active trap.
pub fn room_is_dangerous(d: &Dungeon, id: RoomId) -> bool {
    d.room(id)
        .is_some_and(|r| r.enemies == EnemyState::Active || r.trap == TrapState::Active)
}

/// True if `path` never enters a dangerous room and never traverses a trapped
/// door.
///
/// The **first** room is exempt (the party is already there), but the **last**
/// room is not: a route whose destination is dangerous is unsafe.
pub fn path_is_safe(d: &Dungeon, path: &[RoomId]) -> bool {
    (1..path.len()).all(|i| {
        !room_is_dangerous(d, path[i])
            && !d.doors.iter().any(|door| {
                door.trapped
                    && ((door.from == path[i - 1] && door.target == path[i])
                        || (door.from == path[i] && door.target == path[i - 1]))
            })
    })
}

/// Shortest route from `from` to `to` that uses only safe edges: no trapped
/// doors, no entry into dangerous rooms (the start is exempt, the target is
/// not — so a dangerous `to` yields `None`).
pub fn find_safe_path(d: &Dungeon, from: RoomId, to: RoomId) -> Option<Vec<RoomId>> {
    if from == to {
        return (!room_is_dangerous(d, from)).then_some(vec![from]);
    }

    let mut visited: HashSet<RoomId> = HashSet::new();
    let mut parent: HashMap<RoomId, RoomId> = HashMap::new();
    let mut queue: VecDeque<RoomId> = VecDeque::new();
    queue.push_back(from);
    visited.insert(from);

    while let Some(cur) = queue.pop_front() {
        for door in d
            .doors
            .iter()
            .filter(|door| door.from == cur || door.target == cur)
        {
            let n = if door.from == cur {
                door.target
            } else {
                door.from
            };
            if !visited.contains(&n) && !door.trapped && !room_is_dangerous(d, n) {
                visited.insert(n);
                parent.insert(n, cur);
                queue.push_back(n);
            }
        }
    }

    if !visited.contains(&to) {
        return None;
    }

    let mut path = vec![to];
    let mut cur = to;
    while cur != from {
        cur = parent[&cur];
        path.push(cur);
    }
    path.reverse();
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Direction, EnemyState, FloorId, Room, RoomId, TrapState, VerticalKind};
    use glam::IVec2;

    fn line_dungeon(n: usize) -> Dungeon {
        let mut d = Dungeon::new("line");
        let mut cur = d.entrance;
        for _ in 0..n {
            cur = d.add_horizontal_door(cur, Direction::E).unwrap();
        }
        d
    }

    #[test]
    fn path_to_entrance_from_entrance() {
        let d = line_dungeon(3);
        assert_eq!(path_to_entrance(&d, d.entrance), Some(vec![d.entrance]));
    }

    #[test]
    fn path_to_entrance_walks_parents() {
        let d = line_dungeon(3);
        let c = d.rooms.iter().map(|r| r.id).max().unwrap();
        let p = path_to_entrance(&d, c).unwrap();
        assert_eq!(p.first(), Some(&c));
        assert_eq!(p.last(), Some(&d.entrance));
        assert_eq!(p.len(), 4);
    }

    #[test]
    fn find_path_is_shortest() {
        let d = line_dungeon(3);
        let ids: Vec<RoomId> = d.rooms.iter().map(|r| r.id).collect();
        let from = ids[0];
        let to = ids[3];
        let p = find_path(&d, from, to).unwrap();
        assert_eq!(p.len(), 4);
        assert_eq!(p.first(), Some(&from));
        assert_eq!(p.last(), Some(&to));
    }

    #[test]
    fn find_path_same_room() {
        let d = line_dungeon(2);
        assert_eq!(
            find_path(&d, d.entrance, d.entrance),
            Some(vec![d.entrance])
        );
    }

    #[test]
    fn find_path_across_floors() {
        let mut d = Dungeon::new("v");
        let down = d
            .add_vertical_door(d.entrance, false, VerticalKind::Stairs)
            .unwrap();
        assert_eq!(
            find_path(&d, d.entrance, down),
            Some(vec![d.entrance, down])
        );
    }

    #[test]
    fn find_path_routes_through_entrance() {
        let mut d = Dungeon::new("u");
        let east = d.add_horizontal_door(d.entrance, Direction::E).unwrap();
        let west = d.add_horizontal_door(d.entrance, Direction::W).unwrap();
        assert_eq!(
            find_path(&d, east, west),
            Some(vec![east, d.entrance, west])
        );
    }

    #[test]
    fn find_path_unreachable_is_none() {
        let mut d = Dungeon::new("u");
        // An isolated room with no doors is unreachable.
        d.rooms
            .push(Room::new(RoomId(999), FloorId(0), IVec2::new(10, 10), None));
        assert_eq!(find_path(&d, d.entrance, RoomId(999)), None);
    }

    fn make_door_trapped(d: &mut Dungeon, a: RoomId, b: RoomId) {
        d.doors
            .iter_mut()
            .find(|door| {
                (door.from == a && door.target == b) || (door.from == b && door.target == a)
            })
            .unwrap()
            .trapped = true;
    }

    #[test]
    fn path_is_safe_on_clean_path() {
        let d = line_dungeon(3);
        let last = d.rooms.iter().map(|r| r.id).max().unwrap();
        let p = find_path(&d, d.entrance, last).unwrap();
        assert!(path_is_safe(&d, &p));
    }

    #[test]
    fn path_is_unsafe_when_entering_dangerous_room() {
        let mut d = line_dungeon(2);
        let ids: Vec<RoomId> = d.rooms.iter().map(|r| r.id).collect();
        let dangerous = ids[2]; // entrance, room1, room2
        d.room_mut(dangerous).unwrap().enemies = EnemyState::Active;
        let p = find_path(&d, d.entrance, dangerous).unwrap();
        assert!(!path_is_safe(&d, &p));
        // A route that stops before the dangerous room is safe.
        assert!(path_is_safe(&d, &p[..p.len() - 1]));
    }

    #[test]
    fn path_is_unsafe_on_trapped_door() {
        let mut d = line_dungeon(2);
        let ids: Vec<RoomId> = d.rooms.iter().map(|r| r.id).collect();
        make_door_trapped(&mut d, ids[0], ids[1]);
        let p = find_path(&d, d.entrance, ids[1]).unwrap();
        assert!(!path_is_safe(&d, &p));
    }

    #[test]
    fn find_safe_path_finds_detour() {
        // Entrance -> A -> B(dangerous) -> C straight line,
        // with a parallel detour row below connected at both ends.
        let mut d = Dungeon::new("safe");
        let e = d.entrance;
        let a = d.add_horizontal_door(e, Direction::E).unwrap();
        let b = d.add_horizontal_door(a, Direction::E).unwrap();
        let c = d.add_horizontal_door(b, Direction::E).unwrap();
        let x = d.add_horizontal_door(e, Direction::S).unwrap();
        let y = d.add_horizontal_door(x, Direction::E).unwrap();
        let z = d.add_horizontal_door(y, Direction::E).unwrap();
        let w = d.add_horizontal_door(z, Direction::E).unwrap();
        d.add_horizontal_door(w, Direction::N).unwrap();

        d.room_mut(b).unwrap().enemies = EnemyState::Active;

        // The direct route is shortest but unsafe.
        let direct = find_path(&d, e, c).unwrap();
        assert!(!path_is_safe(&d, &direct));

        // The safe route goes around.
        let safe = find_safe_path(&d, e, c).unwrap();
        assert_eq!(safe, vec![e, x, y, z, w, c]);
        assert!(path_is_safe(&d, &safe));
    }

    #[test]
    fn find_safe_path_avoids_trapped_door() {
        let mut d = Dungeon::new("safe2");
        let e = d.entrance;
        // Detour row: e -> a -> b -> c -> y (4 doors).
        let a = d.add_horizontal_door(e, Direction::E).unwrap();
        let b = d.add_horizontal_door(a, Direction::E).unwrap();
        let c = d.add_horizontal_door(b, Direction::S).unwrap();
        let y = d.add_horizontal_door(c, Direction::W).unwrap();
        // Short direct route: e -> direct -> y (2 doors), then trapped.
        let direct = d.add_horizontal_door(e, Direction::S).unwrap();
        d.add_horizontal_door(direct, Direction::E).unwrap();

        make_door_trapped(&mut d, e, direct);
        let direct_path = find_path(&d, e, y).unwrap();
        assert!(!path_is_safe(&d, &direct_path));
        let safe = find_safe_path(&d, e, y).unwrap();
        assert_eq!(safe, vec![e, a, b, c, y]);
        assert!(path_is_safe(&d, &safe));
    }

    #[test]
    fn find_safe_path_none_when_target_dangerous() {
        let mut d = line_dungeon(2);
        let last = d.rooms.iter().map(|r| r.id).max().unwrap();
        d.room_mut(last).unwrap().trap = TrapState::Active;
        assert_eq!(find_safe_path(&d, d.entrance, last), None);
    }

    #[test]
    fn find_safe_path_none_when_no_safe_route() {
        // 2x2 ring: e -> a -> b and e -> corner -> b.
        let mut d = Dungeon::new("safe3");
        let e = d.entrance;
        let a = d.add_horizontal_door(e, Direction::E).unwrap();
        let corner = d.add_horizontal_door(e, Direction::S).unwrap();
        let b = d.add_horizontal_door(corner, Direction::E).unwrap();
        d.add_horizontal_door(a, Direction::S).unwrap();
        // Both routes to b are blocked: the direct door is trapped, the
        // other row is dangerous.
        make_door_trapped(&mut d, e, corner);
        d.room_mut(a).unwrap().enemies = EnemyState::Active;
        assert_eq!(find_safe_path(&d, e, b), None);
    }
}
