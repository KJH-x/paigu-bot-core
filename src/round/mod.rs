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
