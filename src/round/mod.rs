use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RoundPhase {
    Phase0,
    PhaseI,
    PhaseII,
    PhaseIII,
    Settling,
    Locked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseWindow {
    pub phase: RoundPhase,
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ItemClass {
    A,
    B,
}

impl ItemClass {
    /// `"A"` / `"a"` → A，`"B"` / `"b"` → B，其余为 `None`。
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "A" => Some(ItemClass::A),
            "B" => Some(ItemClass::B),
            _ => None,
        }
    }
}

pub fn phase_at(windows: &[PhaseWindow], timestamp_ms: i64) -> Option<RoundPhase> {
    windows
        .iter()
        .find(|w| timestamp_ms >= w.start_ms && timestamp_ms < w.end_ms)
        .map(|w| w.phase)
}

pub fn can_claim(phase: RoundPhase, class: ItemClass, is_priority: bool) -> bool {
    match phase {
        RoundPhase::Phase0 => false,
        RoundPhase::PhaseI => class == ItemClass::B,
        RoundPhase::PhaseII => match class {
            ItemClass::B => true,
            ItemClass::A => is_priority,
        },
        RoundPhase::PhaseIII | RoundPhase::Settling => true,
        RoundPhase::Locked => false,
    }
}

pub fn can_cancel(phase: RoundPhase, class: ItemClass, is_priority: bool) -> bool {
    can_claim(phase, class, is_priority)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHASES: [RoundPhase; 6] = [
        RoundPhase::Phase0,
        RoundPhase::PhaseI,
        RoundPhase::PhaseII,
        RoundPhase::PhaseIII,
        RoundPhase::Settling,
        RoundPhase::Locked,
    ];
    const CLASSES: [ItemClass; 2] = [ItemClass::A, ItemClass::B];

    #[test]
    fn phase_at_uses_half_open_windows() {
        let windows = vec![
            PhaseWindow {
                phase: RoundPhase::Phase0,
                start_ms: 0,
                end_ms: 100,
            },
            PhaseWindow {
                phase: RoundPhase::PhaseI,
                start_ms: 100,
                end_ms: 200,
            },
        ];
        assert_eq!(phase_at(&windows, -1), None);
        assert_eq!(phase_at(&windows, 0), Some(RoundPhase::Phase0));
        assert_eq!(phase_at(&windows, 99), Some(RoundPhase::Phase0));
        assert_eq!(phase_at(&windows, 100), Some(RoundPhase::PhaseI));
        assert_eq!(phase_at(&windows, 199), Some(RoundPhase::PhaseI));
        assert_eq!(phase_at(&windows, 200), None);
        assert_eq!(phase_at(&[], 50), None);
    }

    #[test]
    fn item_class_parse() {
        assert_eq!(ItemClass::parse("A"), Some(ItemClass::A));
        assert_eq!(ItemClass::parse(" b "), Some(ItemClass::B));
        assert_eq!(ItemClass::parse("C"), None);
        assert_eq!(ItemClass::parse(""), None);
    }

    #[test]
    fn can_claim_matrix_is_exhaustive() {
        // (phase, class, is_priority) -> expected
        let cases: &[(RoundPhase, ItemClass, bool, bool)] = &[
            (RoundPhase::Phase0, ItemClass::A, false, false),
            (RoundPhase::Phase0, ItemClass::A, true, false),
            (RoundPhase::Phase0, ItemClass::B, false, false),
            (RoundPhase::Phase0, ItemClass::B, true, false),
            (RoundPhase::PhaseI, ItemClass::A, false, false),
            (RoundPhase::PhaseI, ItemClass::A, true, false),
            (RoundPhase::PhaseI, ItemClass::B, false, true),
            (RoundPhase::PhaseI, ItemClass::B, true, true),
            (RoundPhase::PhaseII, ItemClass::A, false, false),
            (RoundPhase::PhaseII, ItemClass::A, true, true),
            (RoundPhase::PhaseII, ItemClass::B, false, true),
            (RoundPhase::PhaseII, ItemClass::B, true, true),
            (RoundPhase::PhaseIII, ItemClass::A, false, true),
            (RoundPhase::PhaseIII, ItemClass::A, true, true),
            (RoundPhase::PhaseIII, ItemClass::B, false, true),
            (RoundPhase::PhaseIII, ItemClass::B, true, true),
            (RoundPhase::Settling, ItemClass::A, false, true),
            (RoundPhase::Settling, ItemClass::A, true, true),
            (RoundPhase::Settling, ItemClass::B, false, true),
            (RoundPhase::Settling, ItemClass::B, true, true),
            (RoundPhase::Locked, ItemClass::A, false, false),
            (RoundPhase::Locked, ItemClass::A, true, false),
            (RoundPhase::Locked, ItemClass::B, false, false),
            (RoundPhase::Locked, ItemClass::B, true, false),
        ];
        for (phase, class, is_priority, expected) in cases {
            assert_eq!(
                can_claim(*phase, *class, *is_priority),
                *expected,
                "can_claim({phase:?}, {class:?}, {is_priority})"
            );
        }
        // 覆盖所有 phase × class × is_priority 组合。
        assert_eq!(cases.len(), PHASES.len() * CLASSES.len() * 2);
    }

    #[test]
    fn can_cancel_matches_can_claim_cell_by_cell() {
        for phase in PHASES {
            for class in CLASSES {
                for is_priority in [false, true] {
                    assert_eq!(
                        can_cancel(phase, class, is_priority),
                        can_claim(phase, class, is_priority),
                        "can_cancel({phase:?}, {class:?}, {is_priority})"
                    );
                }
            }
        }
    }

    #[test]
    fn locked_rejects_everything() {
        for class in CLASSES {
            for is_priority in [false, true] {
                assert!(!can_claim(RoundPhase::Locked, class, is_priority));
                assert!(!can_cancel(RoundPhase::Locked, class, is_priority));
            }
        }
    }
}
