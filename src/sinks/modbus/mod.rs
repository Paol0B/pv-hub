pub mod frame;
pub mod server;

use crate::catalog::{catalog, RegKind};
use crate::config::WordOrder;
use crate::model::SolarState;
use chrono::{DateTime, Utc};

/// Split a 32-bit value into two 16-bit registers honoring word order.
fn words_from_u32(raw: u32, order: WordOrder) -> [u16; 2] {
    let hi = (raw >> 16) as u16;
    let lo = (raw & 0xFFFF) as u16;
    match order {
        WordOrder::Abcd => [hi, lo],
        WordOrder::Cdab => [lo, hi],
    }
}

/// Build the full Modbus register bank from the catalog + current state.
pub fn build_bank(state: &SolarState, now: DateTime<Utc>, order: WordOrder) -> Vec<u16> {
    let mut bank = vec![0u16; crate::catalog::bank_words()];
    for d in catalog() {
        let Some(v) = state.value(d.metric, now) else {
            continue;
        };
        let words = match d.kind {
            RegKind::F32 => words_from_u32((v as f32).to_bits(), order),
            RegKind::U32 => words_from_u32(v as u32, order),
        };
        let base = d.register as usize;
        bank[base] = words[0];
        bank[base + 1] = words[1];
    }
    bank
}

/// Decode two registers back to f32 (used by tests / clients).
pub fn f32_from_words(w0: u16, w1: u16, order: WordOrder) -> f32 {
    let (hi, lo) = match order {
        WordOrder::Abcd => (w0, w1),
        WordOrder::Cdab => (w1, w0),
    };
    f32::from_bits(((hi as u32) << 16) | lo as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::def_for;
    use crate::model::Metric;

    fn approx(a: f32, b: f32, eps: f32) {
        assert!((a - b).abs() < eps, "{a} != {b}");
    }

    #[test]
    fn f32_roundtrip_abcd() {
        let mut s = SolarState::default();
        s.set(Metric::Ghi, 812.5);
        let now = Utc::now();
        let bank = build_bank(&s, now, WordOrder::Abcd);
        let reg = def_for(Metric::Ghi).unwrap().register as usize;
        approx(f32_from_words(bank[reg], bank[reg + 1], WordOrder::Abcd), 812.5, 1e-3);
    }

    #[test]
    fn f32_roundtrip_cdab_swaps_words() {
        let mut s = SolarState::default();
        s.set(Metric::Dni, 690.0);
        let now = Utc::now();
        let reg = def_for(Metric::Dni).unwrap().register as usize;
        let abcd = build_bank(&s, now, WordOrder::Abcd);
        let cdab = build_bank(&s, now, WordOrder::Cdab);
        assert_eq!(abcd[reg], cdab[reg + 1]);
        assert_eq!(abcd[reg + 1], cdab[reg]);
        approx(f32_from_words(cdab[reg], cdab[reg + 1], WordOrder::Cdab), 690.0, 1e-3);
    }

    #[test]
    fn missing_metric_stays_zero() {
        let s = SolarState::default();
        let now = Utc::now();
        let bank = build_bank(&s, now, WordOrder::Abcd);
        let reg = def_for(Metric::Ghi).unwrap().register as usize;
        assert_eq!(bank[reg], 0);
        assert_eq!(bank[reg + 1], 0);
    }
}
