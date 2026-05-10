use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MoneyCents(pub i64);

impl MoneyCents {
    pub const fn zero() -> Self {
        Self(0)
    }

    pub fn from_yuan(yuan: i64) -> Self {
        Self(yuan * 100)
    }

    pub fn from_yuan_float(yuan: f64) -> Self {
        Self((yuan * 100.0).round() as i64)
    }

    pub fn to_yuan_float(self) -> f64 {
        self.0 as f64 / 100.0
    }

    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        self.0.checked_add(rhs.0).map(Self)
    }

    pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        self.0.checked_sub(rhs.0).map(Self)
    }

    pub fn checked_mul_i64(self, n: i64) -> Option<Self> {
        self.0.checked_mul(n).map(Self)
    }

    pub fn checked_mul_u32(self, n: u32) -> Option<Self> {
        self.0.checked_mul(n as i64).map(Self)
    }

    pub fn checked_div_i64(self, n: i64) -> Option<Self> {
        if n == 0 {
            None
        } else {
            Some(Self(self.0 / n))
        }
    }

    pub fn as_cents(self) -> i64 {
        self.0
    }

    pub fn format_yuan(self) -> String {
        let yuan = self.0 / 100;
        let cents = (self.0 % 100).abs();
        if self.0 < 0 {
            format!("-{}.{:02}", yuan.abs(), cents)
        } else {
            format!("{}.{:02}", yuan, cents)
        }
    }
}
