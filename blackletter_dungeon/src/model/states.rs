/// Whether enemies are present in a room, from the party's perspective.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum EnemyState {
    /// No information yet.
    #[default]
    Unknown,
    /// Enemies are present and not yet dealt with.
    Active,
    /// The room has been cleared of enemies.
    Cleared,
}

impl EnemyState {
    /// Advance to the next state (used by the cycle hotkey).
    pub const fn cycle(self) -> Self {
        match self {
            EnemyState::Unknown => EnemyState::Active,
            EnemyState::Active => EnemyState::Cleared,
            EnemyState::Cleared => EnemyState::Unknown,
        }
    }

    /// Go back one state (used by right-clicking the cycle button).
    pub const fn cycle_back(self) -> Self {
        match self {
            EnemyState::Unknown => EnemyState::Cleared,
            EnemyState::Active => EnemyState::Unknown,
            EnemyState::Cleared => EnemyState::Active,
        }
    }

    /// All states, in cycle order.
    pub const ALL: [EnemyState; 3] = [EnemyState::Unknown, EnemyState::Active, EnemyState::Cleared];

    pub const fn label(self) -> &'static str {
        match self {
            EnemyState::Unknown => "unknown",
            EnemyState::Active => "active",
            EnemyState::Cleared => "cleared",
        }
    }
}

/// Whether a room has a trap, and its state.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum TrapState {
    /// No trap in the room.
    #[default]
    None,
    /// A trap is present and armed.
    Active,
    /// A trap was here but has been disarmed.
    Disarmed,
}

impl TrapState {
    /// Advance to the next state (used by the cycle hotkey).
    pub const fn cycle(self) -> Self {
        match self {
            TrapState::None => TrapState::Active,
            TrapState::Active => TrapState::Disarmed,
            TrapState::Disarmed => TrapState::None,
        }
    }

    /// Go back one state (used by right-clicking the cycle button).
    pub const fn cycle_back(self) -> Self {
        match self {
            TrapState::None => TrapState::Disarmed,
            TrapState::Active => TrapState::None,
            TrapState::Disarmed => TrapState::Active,
        }
    }

    /// All states, in cycle order.
    pub const ALL: [TrapState; 3] = [TrapState::None, TrapState::Active, TrapState::Disarmed];

    pub const fn label(self) -> &'static str {
        match self {
            TrapState::None => "none",
            TrapState::Active => "active",
            TrapState::Disarmed => "disarmed",
        }
    }
}

/// What is known about loot in a room.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum LootState {
    /// No information yet.
    #[default]
    Unknown,
    /// Loot is present and not yet taken.
    Present,
    /// The party has taken the loot.
    Taken,
    /// Confirmed to have no loot.
    None,
}

impl LootState {
    /// Advance to the next state (used by the cycle hotkey).
    pub const fn cycle(self) -> Self {
        match self {
            LootState::Unknown => LootState::Present,
            LootState::Present => LootState::Taken,
            LootState::Taken => LootState::None,
            LootState::None => LootState::Unknown,
        }
    }

    /// Go back one state (used by right-clicking the cycle button).
    pub const fn cycle_back(self) -> Self {
        match self {
            LootState::Unknown => LootState::None,
            LootState::Present => LootState::Unknown,
            LootState::Taken => LootState::Present,
            LootState::None => LootState::Taken,
        }
    }

    /// All states, in cycle order.
    pub const ALL: [LootState; 4] = [
        LootState::Unknown,
        LootState::Present,
        LootState::Taken,
        LootState::None,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            LootState::Unknown => "unknown",
            LootState::Present => "loot",
            LootState::Taken => "taken",
            LootState::None => "none",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trap_state_cycles_to_start() {
        let mut s = TrapState::None;
        for _ in 0..3 {
            s = s.cycle();
        }
        assert_eq!(s, TrapState::None);
        // Forward then backward is the identity.
        for t in TrapState::ALL {
            assert_eq!(t.cycle().cycle_back(), t);
        }
    }

    #[test]
    fn enemy_state_cycles_to_start() {
        let mut s = EnemyState::Unknown;
        for _ in 0..3 {
            s = s.cycle();
        }
        assert_eq!(s, EnemyState::Unknown);
    }

    #[test]
    fn loot_state_cycles_to_start() {
        let mut s = LootState::Unknown;
        for _ in 0..4 {
            s = s.cycle();
        }
        assert_eq!(s, LootState::Unknown);
    }
}
