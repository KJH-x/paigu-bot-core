use crate::domain::money::MoneyCents;

pub fn format_money(m: MoneyCents) -> String {
    m.format_yuan()
}
