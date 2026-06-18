//! The composed engine: feed + equity + InsuredRiskEngine behind one tick().

use crate::convert::{equity_to_engine, feed_to_engine, ConvertCfg, EngineGates, EngineInputs};
use percolator_equity::equity::{EquityCfg, EquityEngine};
use percolator_equity::marking::NameCfg;
use percolator_equity::session::{CalendarState, Liveness};
use percolator::LiquidationPolicy;
use percolator_feed::{FeedCfg, FeedTick, RiskFeed};
use percolator_insurance::{
    InsuredRiskEngine, PremiumParams, Result as InsResult, RiskParams, MAX_ACCOUNTS,
};

/// Number of price venues the composed feed consumes.
pub const N_VENUES: usize = 4;

/// One venue observation fed to the price feed each tick.
#[derive(Clone, Copy)]
pub struct VenueObs {
    pub venue: usize,
    pub price: u64,
    pub depth: u64,
}

pub struct EngineCfg {
    pub risk_params: RiskParams,
    pub premium_params: PremiumParams,
    pub feed_cfg: FeedCfg,
    pub equity_cfg: EquityCfg,
    pub name_cfg: NameCfg,
    pub convert_cfg: ConvertCfg,
    pub init_price: u64,
}

#[derive(Clone, Copy)]
pub struct EngineState {
    pub tick: FeedTick,
    pub inputs: EngineInputs,
    pub gates: EngineGates,
    pub now_slot: u64,
}

pub struct PercolatorEngine {
    pub insurer: InsuredRiskEngine,
    feed: RiskFeed<N_VENUES>,
    equity: EquityEngine,
    convert_cfg: ConvertCfg,
    last_state: EngineState,
    last_ts: u64,
    reconcile_enabled: bool,
}

impl PercolatorEngine {
    pub fn new(cfg: EngineCfg, init_slot: u64) -> Self {
        let insurer =
            InsuredRiskEngine::new(cfg.risk_params, cfg.premium_params, init_slot, cfg.init_price)
                .expect("valid engine params");
        let feed = RiskFeed::<N_VENUES>::new(cfg.feed_cfg, cfg.init_price);
        let equity = EquityEngine::new(cfg.equity_cfg, cfg.name_cfg);
        let init_tick = FeedTick {
            fast_price: cfg.init_price,
            extraction_price: cfg.init_price,
            funding_e9: 0,
            confidence: crate::convert::CONF,
            extraction_gate: crate::convert::CONF,
            vol_mult: 1_000_000_000,
        };
        let inputs = feed_to_engine(&init_tick, &cfg.convert_cfg);
        let gates = EngineGates {
            allow_open: true,
            margin_mult: 1_000_000_000,
            reopen_gap_margin: 0,
            extraction_warmup: 0,
            mark_underlying: true,
        };
        Self {
            insurer,
            feed,
            equity,
            convert_cfg: cfg.convert_cfg,
            last_state: EngineState { tick: init_tick, inputs, gates, now_slot: init_slot },
            last_ts: 0,
            reconcile_enabled: true,
        }
    }

    /// Toggle the pool/fund reconciliation step. Default on. Turning it off models
    /// an integrator that collects premium but forgets to reconcile — used to prove
    /// the solvency invariant's guard fires (see `invariants` + `redteam`).
    pub fn set_reconcile_enabled(&mut self, v: bool) {
        self.reconcile_enabled = v;
    }

    /// Run the insurer's premium lifecycle: collect accrued premium from every
    /// active account into the fund (recording the real delta to the pool), then
    /// reconcile the pool against the actual fund balance. This is the keeper crank
    /// the composition runs each tick — without it the insurance layer is inert.
    fn run_maintenance(&mut self, now_slot: u64) {
        for idx in 0..MAX_ACCOUNTS as u16 {
            let _ = self.insurer.collect_accrued_premium(idx, now_slot);
        }
        if self.reconcile_enabled {
            self.insurer.reconcile_pool();
        }
    }

    /// Advance one tick: feed observations -> FeedTick -> equity -> derived inputs.
    pub fn tick(
        &mut self,
        obs: &[VenueObs],
        cal: CalendarState,
        now_slot: u64,
        now_ts: u64,
    ) -> EngineState {
        for o in obs {
            self.feed.observe(o.venue, o.price, now_ts, o.depth);
        }
        let dt = now_ts.saturating_sub(self.last_ts).max(1);
        self.last_ts = now_ts;
        let perp_mark = self.last_state.inputs.oracle_price;
        let tick = self.feed.tick(now_ts, dt, perp_mark);

        // No fresh observations ⇒ the underlying is going stale (drives the
        // data-driven Closed/Halted detection in equity).
        let staleness = if obs.is_empty() { dt as u32 } else { 0 };
        let live = Liveness { underlying_staleness_slots: staleness, confidence: tick.confidence };
        let eq = self.equity.update(cal, live);

        let inputs = feed_to_engine(&tick, &self.convert_cfg);
        let gates = equity_to_engine(&eq);
        self.insurer.accrue(now_slot);
        self.run_maintenance(now_slot);
        let st = EngineState { tick, inputs, gates, now_slot };
        self.last_state = st;
        st
    }

    pub fn state(&self) -> EngineState {
        self.last_state
    }

    /// Seed an account with capital (LP or trader). Mirrors sim's deposit.
    pub fn deposit(&mut self, idx: u16, amount: u128, now_slot: u64) -> InsResult<()> {
        self.insurer.deposit(idx, amount, now_slot)
    }

    /// Open/increase a position a-vs-b. Returns Ok(false) (a no-op) when the equity
    /// gates forbid risk increase — naked opens are blocked in Closed/Halted/
    /// PreEvent or for a non-Admissible name. Never blocks liquidation/reduction.
    pub fn open(&mut self, a: u16, b: u16, size_q: i128, now_slot: u64) -> InsResult<bool> {
        if !self.last_state.gates.allow_open {
            return Ok(false);
        }
        let inp = self.last_state.inputs;
        self.insurer.execute_trade(
            a,
            b,
            inp.oracle_price,
            now_slot,
            size_q,
            inp.oracle_price,
            inp.funding_e9,
            inp.admit_h_min,
            inp.admit_h_max,
            None,
        )?;
        Ok(true)
    }

    /// Liquidate an account at the fast price. Always allowed (risk-reducing).
    pub fn liquidate(&mut self, idx: u16, now_slot: u64) -> InsResult<bool> {
        let inp = self.last_state.inputs;
        self.insurer.liquidate(
            idx,
            now_slot,
            inp.oracle_price,
            LiquidationPolicy::FullClose,
            inp.funding_e9,
            inp.admit_h_min,
            inp.admit_h_max,
            None,
        )
    }

    /// Withdraw at the persistence-confirmed extraction price + warmup band.
    pub fn withdraw(&mut self, idx: u16, amount: u128, now_slot: u64) -> InsResult<()> {
        let inp = self.last_state.inputs;
        self.insurer.withdraw(
            idx,
            amount,
            inp.extraction_price,
            now_slot,
            inp.funding_e9,
            inp.admit_h_min,
            inp.admit_h_max,
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::*;

    fn cfg() -> EngineCfg {
        EngineCfg {
            risk_params: risk_params(),
            premium_params: premium_params(),
            feed_cfg: feed_cfg(),
            equity_cfg: equity_cfg(),
            name_cfg: name_cfg(),
            convert_cfg: ConvertCfg { h_max_cap: 100 },
            init_price: 50_000,
        }
    }

    #[test]
    fn tick_produces_sane_state() {
        let mut e = PercolatorEngine::new(cfg(), 0);
        let obs = [
            VenueObs { venue: 0, price: 50_000, depth: 10 },
            VenueObs { venue: 1, price: 50_010, depth: 10 },
            VenueObs { venue: 2, price: 49_990, depth: 10 },
        ];
        let st = e.tick(&obs, CalendarState::Open, 1, 1);
        assert!(st.inputs.oracle_price >= 1);
        assert!(st.gates.allow_open); // Open + proxied
    }

    #[test]
    fn closed_session_gates_opens_in_state() {
        let mut e = PercolatorEngine::new(cfg(), 0);
        let st = e.tick(&[], CalendarState::Closed, 1, 1);
        assert!(!st.gates.allow_open);
    }

    fn seeded() -> PercolatorEngine {
        let mut e = PercolatorEngine::new(cfg(), 0);
        let _ = e.deposit(0, 1_000_000_000, 0); // LP
        let _ = e.deposit(1, 1_000_000, 0); // trader
        e
    }

    #[test]
    fn open_rejected_when_session_gates_it() {
        let mut e = seeded();
        e.tick(&[], CalendarState::Closed, 1, 1); // Closed -> allow_open=false
        let opened = e.open(1, 0, 1000, 2).unwrap();
        assert!(!opened); // gate held, no trade
    }

    #[test]
    fn open_allowed_when_open_session() {
        let mut e = seeded();
        let obs = [
            VenueObs { venue: 0, price: 50_000, depth: 100 },
            VenueObs { venue: 1, price: 50_000, depth: 100 },
            VenueObs { venue: 2, price: 50_000, depth: 100 },
        ];
        e.tick(&obs, CalendarState::Open, 1, 1);
        let opened = e.open(1, 0, 1000, 2).unwrap();
        assert!(opened);
    }

    #[test]
    fn liquidate_runs_even_when_opens_gated() {
        let mut e = seeded();
        e.tick(&[], CalendarState::Closed, 1, 1); // opens gated
        let _ = e.liquidate(1, 2); // callable; gate does not block liquidation
    }

    #[test]
    fn premium_lifecycle_runs_pool_collects() {
        // With the maintenance pass wired in, an open position over many slots must
        // actually move premium into the fund/pool — proving the insurance layer is
        // no longer inert.
        let mut e = seeded();
        let obs = [
            VenueObs { venue: 0, price: 50_000, depth: 100 },
            VenueObs { venue: 1, price: 50_000, depth: 100 },
            VenueObs { venue: 2, price: 50_000, depth: 100 },
        ];
        e.tick(&obs, CalendarState::Open, 1, 1);
        assert!(e.open(1, 0, 5000, 1).unwrap());
        for s in 2..50 {
            e.tick(&obs, CalendarState::Open, s, s);
        }
        assert!(
            e.insurer.pool.total_collected > 0,
            "premium never collected -> insurance still inert"
        );
    }
}
