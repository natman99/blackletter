use glam::IVec2;

/// The eight compass directions a room can connect to (orthogonal + diagonal).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Direction {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

impl Direction {
    /// Grid offset for this direction. `y` grows downward (screen rows), so
    /// `N` moves up and `S` moves down — matching vim `k`/`j`.
    pub const fn offset(self) -> IVec2 {
        match self {
            Direction::N => IVec2::new(0, -1),
            Direction::NE => IVec2::new(1, -1),
            Direction::E => IVec2::new(1, 0),
            Direction::SE => IVec2::new(1, 1),
            Direction::S => IVec2::new(0, 1),
            Direction::SW => IVec2::new(-1, 1),
            Direction::W => IVec2::new(-1, 0),
            Direction::NW => IVec2::new(-1, -1),
        }
    }

    /// The direction directly opposite.
    pub const fn opposite(self) -> Direction {
        match self {
            Direction::N => Direction::S,
            Direction::S => Direction::N,
            Direction::E => Direction::W,
            Direction::W => Direction::E,
            Direction::NE => Direction::SW,
            Direction::SW => Direction::NE,
            Direction::NW => Direction::SE,
            Direction::SE => Direction::NW,
        }
    }

    /// Map a unit grid delta to a direction. Returns `None` if the delta is not
    /// one of the eight adjacent cells.
    pub fn from_delta(delta: IVec2) -> Option<Direction> {
        Direction::ALL.into_iter().find(|d| d.offset() == delta)
    }

    /// All eight directions.
    pub const ALL: [Direction; 8] = [
        Direction::N,
        Direction::NE,
        Direction::E,
        Direction::SE,
        Direction::S,
        Direction::SW,
        Direction::W,
        Direction::NW,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Direction::N => "N",
            Direction::NE => "NE",
            Direction::E => "E",
            Direction::SE => "SE",
            Direction::S => "S",
            Direction::SW => "SW",
            Direction::W => "W",
            Direction::NW => "NW",
        }
    }
}

/// How a vertical (between-floor) connection works. Cosmetic for now — the
/// requirement is that the *visual* means of changing floors doesn't matter.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum VerticalKind {
    #[default]
    Stairs,
    Ladder,
    Shaft,
    Other,
}

impl VerticalKind {
    pub const fn label(self) -> &'static str {
        match self {
            VerticalKind::Stairs => "stairs",
            VerticalKind::Ladder => "ladder",
            VerticalKind::Shaft => "shaft",
            VerticalKind::Other => "passage",
        }
    }
}

/// How two rooms are connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Connection {
    /// Same floor, one of the eight grid directions. The target room sits at
    /// `from.position + direction.offset()`.
    Horizontal(Direction),
    /// Between two floors. Up/down is derived from the rooms' floor numbers.
    Vertical { kind: VerticalKind },
}

impl Connection {
    pub fn is_horizontal(self) -> bool {
        matches!(self, Connection::Horizontal(_))
    }

    pub fn is_vertical(self) -> bool {
        matches!(self, Connection::Vertical { .. })
    }

    /// The connection as seen from the other side.
    pub fn reverse(self) -> Connection {
        match self {
            Connection::Horizontal(d) => Connection::Horizontal(d.opposite()),
            Connection::Vertical { kind } => Connection::Vertical { kind },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_are_single_steps() {
        for d in Direction::ALL {
            let o = d.offset();
            assert!(o.x.abs() <= 1 && o.y.abs() <= 1);
            assert!(o.x != 0 || o.y != 0);
        }
    }

    #[test]
    fn opposite_is_involutive() {
        for d in Direction::ALL {
            assert_eq!(d.opposite().opposite(), d);
            assert_ne!(d.opposite(), d);
        }
    }

    #[test]
    fn from_delta_roundtrip() {
        for d in Direction::ALL {
            assert_eq!(Direction::from_delta(d.offset()), Some(d));
        }
        assert_eq!(Direction::from_delta(IVec2::ZERO), None);
        assert_eq!(Direction::from_delta(IVec2::new(2, 0)), None);
        assert_eq!(Direction::from_delta(IVec2::new(2, 1)), None);
    }

    #[test]
    fn connection_reverse() {
        assert_eq!(
            Connection::Horizontal(Direction::E).reverse(),
            Connection::Horizontal(Direction::W)
        );
        assert_eq!(
            Connection::Vertical {
                kind: VerticalKind::Stairs
            }
            .reverse(),
            Connection::Vertical {
                kind: VerticalKind::Stairs
            }
        );
    }
}
