//! Adapters: FeedTick -> engine price/funding/admission inputs; EquityRiskState ->
//! margin/gate inputs. Pure functions; scale reconciliation + clamps live here.

use percolator_equity::equity::EquityRiskState;
use percolator_equity::marking::{Admission, MarkSource};
use percolator_feed::FeedTick;

/// Engine price scale ceiling (percolator MAX_ORACLE_PRICE).
pub const MAX_ORACLE_PRICE: u64 = 1_000_000_000_000;
/// Confidence full-scale (feed/equity CONF).
pub const CONF: u32 = 1_000_000;

#[derive(Clone, Copy)]
pub struct ConvertCfg {
    pub h_max_cap: u64, // engine RiskParams.h_max (warmup band ceiling)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EngineInputs {
    pub oracle_price: u64,     // fast_price, clamped to range -> liquidation/mark
    pub extraction_price: u64, // persistence-confirmed -> withdraw/realize
    pub funding_e9: i128,
    pub admit_h_min: u64,
    pub admit_h_max: u64, // lower confidence -> larger warmup
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EngineGates {
    pub allow_open: bool,
    pub margin_mult: u64,
    pub reopen_gap_margin: u64,
    pub extraction_warmup: u32,
    pub mark_underlying: bool, // true = mark to underlying; false = proxy/frozen
}

/// Map a FeedTick to the engine's price/funding/admission inputs. Prices are
/// clamped into the engine's oracle range; lower extraction confidence widens the
/// warmup band so fresh PnL is admitted more slowly on a less-trusted price.
pub fn feed_to_engine(tick: &FeedTick, cfg: &ConvertCfg) -> EngineInputs {
    let clamp = |p: u64| p.min(MAX_ORACLE_PRICE).max(1);
    let gate = tick.extraction_gate.min(CONF) as u64;
    let admit_h_max = cfg.h_max_cap - cfg.h_max_cap * gate / CONF as u64;
    EngineInputs {
        oracle_price: clamp(tick.fast_price),
        extraction_price: clamp(tick.extraction_price),
        funding_e9: tick.funding_e9,
        admit_h_min: 0,
        admit_h_max,
    }
}

/// Map an EquityRiskState to the engine gates. `allow_open` ANDs the asymmetric
/// risk-increase flag with the soundness gate (defense in depth).
pub fn equity_to_engine(st: &EquityRiskState) -> EngineGates {
    EngineGates {
        allow_open: st.risk.allow_risk_increase && st.admission == Admission::Admissible,
        margin_mult: st.risk.margin_mult,
        reopen_gap_margin: st.risk.reopen_gap_margin,
        extraction_warmup: st.risk.extraction_warmup,
        mark_underlying: matches!(st.mark, MarkSource::Underlying),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{equity_cfg, name_cfg};
    use percolator_equity::equity::EquityEngine;
    use percolator_equity::session::{CalendarState, Liveness};

    fn cfg() -> ConvertCfg {
        ConvertCfg { h_max_cap: 100 }
    }

    fn a_tick(gate: u32) -> FeedTick {
        FeedTick {
            fast_price: 50_000,
            extraction_price: 49_900,
            funding_e9: 123,
            confidence: gate,
            extraction_gate: gate,
            vol_mult: 1_000_000_000,
        }
    }

    #[test]
    fn price_clamped_into_engine_range() {
        let mut t = a_tick(CONF);
        t.fast_price = MAX_ORACLE_PRICE + 5;
        let e = feed_to_engine(&t, &cfg());
        assert_eq!(e.oracle_price, MAX_ORACLE_PRICE);
        assert!(e.oracle_price >= 1);
    }

    #[test]
    fn funding_passthrough_is_identity() {
        let e = feed_to_engine(&a_tick(CONF), &cfg());
        assert_eq!(e.funding_e9, 123);
    }

    #[test]
    fn low_confidence_widens_warmup_band() {
        let hi = feed_to_engine(&a_tick(CONF), &cfg()).admit_h_max;
        let lo = feed_to_engine(&a_tick(CONF / 4), &cfg()).admit_h_max;
        assert!(lo > hi); // less confidence -> more warmup
        assert_eq!(hi, 0); // full confidence -> no warmup
    }

    #[test]
    fn equity_gates_open_when_admissible_and_allowed() {
        let mut e = EquityEngine::new(equity_cfg(), name_cfg());
        let st = e.update(
            CalendarState::Open,
            Liveness { underlying_staleness_slots: 0, confidence: CONF },
        );
        let g = equity_to_engine(&st);
        assert!(g.allow_open);
        assert!(g.mark_underlying);
    }

    #[test]
    fn equity_gates_closed_blocks_open() {
        let mut e = EquityEngine::new(equity_cfg(), name_cfg());
        let st = e.update(
            CalendarState::Closed,
            Liveness { underlying_staleness_slots: 10, confidence: CONF },
        );
        let g = equity_to_engine(&st);
        assert!(!g.allow_open);
        assert!(g.reopen_gap_margin > 0);
    }
}
