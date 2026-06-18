//! Cross-crate invariant contract. `check()` returns a Violation (data, not a
//! panic). These are the properties the red-team falsifies.

use crate::engine::PercolatorEngine;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Violation {
    /// Pool accounting identity broke (balance + paid_out != collected, or
    /// paid_out > collected). Self-maintaining via the API — a regression guard
    /// that would only trip on a future engine bug.
    PoolAccounting,
    /// A non-tradeable, non-underlying-marked session (Closed/Halted) is held with
    /// NO gap protection (no reopen buffer, no extraction warmup). Positions would
    /// enter the closed period unprotected against the reopen gap. Breakable by a
    /// surcharge misconfiguration — the red-team's primary target.
    ReopenBufferMissing,
}

/// Check the cross-crate contract against the engine's current state.
pub fn check(eng: &PercolatorEngine) -> Result<(), Violation> {
    // 1. Pool accounting identity (solvency: paid_out <= collected).
    if !eng.insurer.pool.check_invariants() {
        return Err(Violation::PoolAccounting);
    }
    // 2. Gate contract: a gated, proxy/frozen-marked session must hold protection.
    let g = eng.state().gates;
    let unprotected = g.reopen_gap_margin == 0 && g.extraction_warmup == 0;
    if !g.allow_open && !g.mark_underlying && unprotected {
        return Err(Violation::ReopenBufferMissing);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{EngineCfg, PercolatorEngine, VenueObs};
    use crate::fixtures::*;
    use percolator_equity::session::CalendarState;

    fn cfg() -> EngineCfg {
        EngineCfg {
            risk_params: risk_params(),
            premium_params: premium_params(),
            feed_cfg: feed_cfg(),
            equity_cfg: equity_cfg(),
            name_cfg: name_cfg(),
            convert_cfg: crate::convert::ConvertCfg { h_max_cap: 100 },
            init_price: 50_000,
        }
    }

    #[test]
    fn holds_in_normal_operation() {
        let mut e = PercolatorEngine::new(cfg(), 0);
        let obs = [
            VenueObs { venue: 0, price: 50_000, depth: 100 },
            VenueObs { venue: 1, price: 50_000, depth: 100 },
            VenueObs { venue: 2, price: 50_000, depth: 100 },
        ];
        e.tick(&obs, CalendarState::Open, 1, 1);
        assert_eq!(check(&e), Ok(()));
    }

    #[test]
    fn normal_closed_session_holds_buffer() {
        // A correctly-configured Closed session DOES hold a reopen buffer -> Ok.
        let mut e = PercolatorEngine::new(cfg(), 0);
        e.tick(&[], CalendarState::Closed, 1, 1);
        assert_eq!(check(&e), Ok(()));
    }

    #[test]
    fn catches_unprotected_closed_session() {
        // Misconfigure: zero the reopen buffers. A Closed session then gates opens,
        // marks the proxy, but holds NO protection -> the checker must catch it.
        let mut c = cfg();
        c.equity_cfg.surcharge.reopen_gap_margin_closed = 0;
        c.equity_cfg.surcharge.reopen_gap_margin_halted = 0;
        c.equity_cfg.risk.halted_extraction_warmup = 0;
        let mut e = PercolatorEngine::new(c, 0);
        e.tick(&[], CalendarState::Closed, 1, 1);
        assert_eq!(check(&e), Err(Violation::ReopenBufferMissing));
    }
}
