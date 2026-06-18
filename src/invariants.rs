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
    /// The pool's recorded premium claim exceeds the actual insurance fund balance
    /// backing it — the fund cannot honour what the pool thinks it holds. Maintained
    /// by `reconcile_pool`; breaks when reconciliation is skipped while the fund is
    /// drawn down (the phantom-payout / under-reconciliation class).
    Insolvency,
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
    // 3. Solvency: the pool's claim cannot exceed the fund balance backing it.
    let fund = eng.insurer.engine.insurance_fund.balance.get();
    if eng.insurer.pool.balance > fund {
        return Err(Violation::Insolvency);
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

    #[test]
    fn solvency_holds_with_premium_lifecycle_running() {
        // Reconcile on: premium flows and the pool's claim stays backed by the fund.
        let mut e = PercolatorEngine::new(cfg(), 0);
        let _ = e.deposit(0, 1_000_000_000, 0);
        let _ = e.deposit(1, 1_000_000, 0);
        let obs = [
            VenueObs { venue: 0, price: 50_000, depth: 100 },
            VenueObs { venue: 1, price: 50_000, depth: 100 },
            VenueObs { venue: 2, price: 50_000, depth: 100 },
        ];
        e.tick(&obs, CalendarState::Open, 1, 1);
        let _ = e.open(1, 0, 5000, 1);
        for s in 2..20 {
            e.tick(&obs, CalendarState::Open, s, s);
        }
        assert!(e.insurer.pool.total_collected > 0); // lifecycle ran
        assert_eq!(check(&e), Ok(())); // and stayed solvent
    }

    #[test]
    fn catches_insolvency_when_fund_drawn_below_pool_without_reconcile() {
        // The realistic failure: an integrator collects premium but forgets to
        // reconcile, then a fund deficit is paid. With reconcile skipped, that
        // drawdown is never recorded against the pool -> the pool's claim now
        // exceeds the fund -> the guard must catch it.
        let mut e = PercolatorEngine::new(cfg(), 0);
        let _ = e.deposit(0, 1_000_000_000, 0);
        let _ = e.deposit(1, 1_000_000, 0);
        e.set_reconcile_enabled(false);
        let obs = [
            VenueObs { venue: 0, price: 50_000, depth: 100 },
            VenueObs { venue: 1, price: 50_000, depth: 100 },
            VenueObs { venue: 2, price: 50_000, depth: 100 },
        ];
        e.tick(&obs, CalendarState::Open, 1, 1);
        let _ = e.open(1, 0, 5000, 1);
        for s in 2..20 {
            e.tick(&obs, CalendarState::Open, s, s);
        }
        let pool = e.insurer.pool.balance;
        assert!(pool > 0);
        // simulate a fund drawdown (deficit payout) that reconcile would have caught
        e.insurer.engine.insurance_fund.balance.set(pool - 1);
        assert_eq!(check(&e), Err(Violation::Insolvency));
    }
}
