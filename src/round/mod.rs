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

impl RoundPhase {
    /// 稳定的机器可读标识（供 API/前端使用）。
    pub fn as_str(&self) -> &'static str {
        match self {
            RoundPhase::Phase0 => "phase0",
            RoundPhase::PhaseI => "phase1",
            RoundPhase::PhaseII => "phase2",
            RoundPhase::PhaseIII => "phase3",
            RoundPhase::Settling => "settling",
            RoundPhase::Locked => "locked",
        }
    }

    /// 展示用中文标签。
    pub fn label(&self) -> &'static str {
        match self {
            RoundPhase::Phase0 => "开团前",
            RoundPhase::PhaseI => "全量排谷",
            RoundPhase::PhaseII => "优先排谷",
            RoundPhase::PhaseIII => "全员可改",
            RoundPhase::Settling => "结算中",
            RoundPhase::Locked => "已锁定",
        }
    }
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
mod tests;
