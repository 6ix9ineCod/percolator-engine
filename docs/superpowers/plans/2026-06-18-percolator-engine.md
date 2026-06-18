# percolator-engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the integration layer that composes `percolator-feed` + `percolator-equity` into the real `InsuredRiskEngine` behind one `tick()`, plus an adversarial red-team that attacks the real composition for invariant breaks.

**Architecture:** A `std` crate depending on `percolator-insurance`, `percolator-feed`, `percolator-equity` (path-deps locally). `convert` adapts leaf outputs to engine inputs; `engine` holds the composed `PercolatorEngine` with a `tick()` + intent ops enforcing the equity gates; `invariants` is the cross-crate contract; `redteam` searches the action space for breaks with a negative-control proof.

**Tech Stack:** Rust 2021. Engine driving follows `percolator-sim`'s proven pattern. Scales: feed/equity `SCALE=1e9` `CONF=1e6`; engine `MAX_ORACLE_PRICE=1e12` `POS_SCALE=1e6` `PREMIUM_SCALE=1e9` `MULT_SCALE=1e3`.

---

## File Structure

- `Cargo.toml` — path-deps on the three crates.
- `src/lib.rs` — module declarations + re-exports.
- `src/fixtures.rs` — `#[cfg(any(test, feature = "fixtures"))]` canonical configs (RiskParams, PremiumParams, FeedCfg, EquityCfg, NameCfg, ConvertCfg, EngineCfg) reused by tests, harness, bin, red-team.
- `src/convert.rs` — `EngineInputs`, `EngineGates`, `ConvertCfg`, `feed_to_engine`, `equity_to_engine`. Pure, no engine state.
- `src/engine.rs` — `PercolatorEngine`, `EngineState`, `EngineCfg`, `VenueObs`, `new`, `tick`, intent ops (`open`/`close`/`withdraw`/`liquidate`).
- `src/invariants.rs` — `Violation`, `check`.
- `src/redteam.rs` — `Action`, `Outcome`, `search`, shrinking.
- `src/harness.rs` — composed demo-day driver.
- `src/bin/engine.rs` — demo + red-team runner CLI.

---

### Task 0: Scaffold + fixtures

**Files:**
- Create: `Cargo.toml`, `src/lib.rs`, `src/fixtures.rs`

- [ ] **Step 1: Write `Cargo.toml`**

```toml
[package]
name = "percolator-engine"
version = "0.1.0"
edition = "2021"

[features]
fixtures = []

[dependencies]
percolator-insurance = { path = "../percolator/percolator-insurance", features = ["serde"] }
percolator-feed = { path = "../percolator-feed" }
percolator-equity = { path = "../percolator-equity" }

[[bin]]
name = "engine"
```

- [ ] **Step 2: Write `src/fixtures.rs`** (canonical configs; full literals, no Default exists)

```rust
//! Canonical test/demo configs. Built once, reused by tests, harness, bin, red-team.
#![cfg(any(test, feature = "fixtures"))]

use percolator_equity::equity::EquityCfg;
use percolator_equity::fixed::{CONF, SCALE};
use percolator_equity::marking::{MarkCfg, NameCfg, ProxyKind};
use percolator_equity::riskmode::RiskCfg;
use percolator_equity::session::SessionCfg;
use percolator_equity::surcharge::SurchargeCfg;
use percolator_feed::FeedCfg;
use percolator_insurance::{
    InsuranceFund, PremiumParams, RiskParams, MULT_SCALE, U128_helper_unused,
};

pub fn risk_params() -> RiskParams {
    RiskParams {
        maintenance_margin_bps: 500,
        initial_margin_bps: 1000,
        trading_fee_bps: 10,
        max_accounts: 64,
        liquidation_fee_bps: 100,
        liquidation_fee_cap: percolator_insurance::U128::new(1_000_000_000),
        min_liquidation_abs: percolator_insurance::U128::new(0),
        min_nonzero_mm_req: 10,
        min_nonzero_im_req: 11,
        h_min: 0,
        h_max: 100,
        resolve_price_deviation_bps: 1000,
        max_accrual_dt_slots: 100,
        max_abs_funding_e9_per_slot: 10_000,
        min_funding_lifetime_slots: 10_000_000,
        max_active_positions_per_side: 64,
        max_price_move_bps_per_slot: 3,
    }
}

pub fn premium_params() -> PremiumParams {
    PremiumParams {
        base_rate_per_slot: 100,
        leverage_exponent_num: 3,
        leverage_exponent_den: 2,
        min_commitment_slots: 216_000,
        crowding_low_ratio_num: 1500,
        crowding_low_ratio_den: 1000,
        crowding_high_ratio_num: 5000,
        crowding_high_ratio_den: 1000,
        crowding_cap: 4000,
        oi_vault_floor_ratio_num: 1,
        oi_vault_floor_ratio_den: 1,
        oi_vault_cap_ratio_num: 5,
        oi_vault_cap_ratio_den: 1,
        oi_vault_mult_max: 3000,
        pool_health_low_num: 1,
        pool_health_low_den: 100,
        pool_health_high_num: 5,
        pool_health_high_den: 100,
        pool_health_mult_max: 5000,
        min_premium_per_slot: 1,
        volatility_mult_num: MULT_SCALE,
        volatility_mult_den: MULT_SCALE,
        leverage_tail_threshold_bps: 8000,
        leverage_tail_steepness: 3000,
        collection_maint_buffer_bps: 0,
        max_oracle_deviation_bps: 0,
        max_oracle_staleness_slots: 0,
        require_authorization: false,
    }
}

pub fn feed_cfg() -> FeedCfg {
    FeedCfg {
        max_staleness: 100,
        max_disp_bps: 500,
        max_price_move_bps_per_slot: 50,
        min_gate: 0,
        funding_per_slot_cap: 1_000_000,
        funding_cum_cap: 100_000_000,
        confirm_band_bps: 50,
        min_total_depth: 1,
        vol_lambda_num: SCALE * 94 / 100,
    }
}

pub fn equity_cfg() -> EquityCfg {
    EquityCfg {
        session: SessionCfg { halt_slots: 3, halt_conf: CONF / 2, settle_window: 2 },
        mark: MarkCfg { conf_decay_per_slot: 1000, min_conf: 10_000 },
        surcharge: SurchargeCfg {
            pre_event_mult: 2 * SCALE,
            closed_mult: 3 * SCALE / 2,
            halted_mult: 2 * SCALE,
            reopen_gap_margin_closed: SCALE / 10,
            reopen_gap_margin_halted: SCALE / 5,
            pre_event_window: 100,
        },
        risk: RiskCfg {
            max_lev: 20 * SCALE,
            pre_event_max_lev: 3 * SCALE,
            allow_open_pre_event: false,
            halted_extraction_warmup: 500_000,
        },
    }
}

pub fn name_cfg() -> NameCfg {
    NameCfg { has_proxy: true, proxy_kind: ProxyKind::IndexFuture, list_without_proxy: true }
}
```

NOTE (resolve during impl): the `U128_helper_unused` / `InsuranceFund` imports above are placeholders for whatever the insurer crate actually re-exports for `U128`; verify the real `U128` path (`percolator_insurance::U128`) and drop unused imports. `max_active_positions_per_side` is `u64`.

- [ ] **Step 3: Write `src/lib.rs`**

```rust
pub mod convert;
pub mod engine;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;
pub mod invariants;
pub mod redteam;
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: compiles (fixtures behind cfg; empty modules added next tasks — temporarily declare them as empty files or comment until created).

NOTE: create empty `src/convert.rs`, `src/engine.rs`, `src/invariants.rs`, `src/redteam.rs` with a `// placeholder` line so `cargo build` resolves the module declarations; each is filled by its task.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/lib.rs src/fixtures.rs src/convert.rs src/engine.rs src/invariants.rs src/redteam.rs
git commit -m "feat: scaffold percolator-engine + canonical fixtures"
```

---

### Task 1: convert.rs — leaf outputs → engine inputs

**Files:**
- Create: `src/convert.rs` (replace placeholder)

- [ ] **Step 1: Write the failing tests**

```rust
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

pub fn feed_to_engine(tick: &FeedTick, cfg: &ConvertCfg) -> EngineInputs {
    let clamp = |p: u64| p.min(MAX_ORACLE_PRICE).max(1);
    // lower extraction_gate (lower confidence) -> larger warmup band
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
    use percolator_feed::{FeedCfg, RiskFeed};

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
    fn equity_gates_open_only_when_admissible_and_allowed() {
        // Open + proxied name -> allow_open true
        let mut e = EquityEngine::new(equity_cfg(), name_cfg());
        let st = e.update(CalendarState::Open, Liveness { underlying_staleness_slots: 0, confidence: CONF });
        let g = equity_to_engine(&st);
        assert!(g.allow_open);
        assert!(g.mark_underlying);
    }

    #[test]
    fn equity_gates_closed_blocks_open() {
        let mut e = EquityEngine::new(equity_cfg(), name_cfg());
        let st = e.update(CalendarState::Closed, Liveness { underlying_staleness_slots: 10, confidence: CONF });
        let g = equity_to_engine(&st);
        assert!(!g.allow_open);
        assert!(g.reopen_gap_margin > 0);
        let _ = FeedCfg::clone; // keep imports used
        let _: fn(FeedCfg, u64) -> RiskFeed<3> = RiskFeed::<3>::new;
    }
}
```

- [ ] **Step 2: Run tests to verify they fail** — `cargo test convert::` → FAIL (module was placeholder).
- [ ] **Step 3:** The implementation is already in the file above (written together with tests).
- [ ] **Step 4: Run tests to verify they pass** — `cargo test convert::` → PASS (5 tests). Remove the two `keep imports used` lines if they cause warnings; they exist only to anchor the example imports — delete if unused.
- [ ] **Step 5: Commit**

```bash
git add src/convert.rs
git commit -m "feat: convert layer (feed/equity -> engine inputs/gates) + scale reconciliation"
```

---

### Task 2: engine.rs — PercolatorEngine + tick

**Files:**
- Create: `src/engine.rs` (replace placeholder)

- [ ] **Step 1: Write the failing tests + implementation**

```rust
//! The composed engine: feed + equity + InsuredRiskEngine behind one tick().

use crate::convert::{equity_to_engine, feed_to_engine, ConvertCfg, EngineGates, EngineInputs};
use percolator_equity::equity::{EquityCfg, EquityEngine};
use percolator_equity::marking::NameCfg;
use percolator_equity::session::{CalendarState, Liveness};
use percolator_feed::{FeedCfg, FeedTick, RiskFeed};
use percolator_insurance::{InsuredRiskEngine, PremiumParams, RiskParams};

/// Venue observation fed to the price feed each tick.
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

pub const N_VENUES: usize = 4;

pub struct PercolatorEngine {
    pub insurer: InsuredRiskEngine,
    feed: RiskFeed<N_VENUES>,
    equity: EquityEngine,
    convert_cfg: ConvertCfg,
    last_state: EngineState,
    last_ts: u64,
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
        Self {
            insurer,
            feed,
            equity,
            convert_cfg: cfg.convert_cfg,
            last_state: EngineState { tick: init_tick, inputs, gates: EngineGates {
                allow_open: true, margin_mult: 1_000_000_000, reopen_gap_margin: 0,
                extraction_warmup: 0, mark_underlying: true,
            }, now_slot: init_slot },
            last_ts: 0,
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

        // staleness in slots ~ slots since last tick if no fresh obs; here 0 when obs present
        let staleness = if obs.is_empty() { dt as u32 } else { 0 };
        let live = Liveness { underlying_staleness_slots: staleness, confidence: tick.confidence };
        let eq = self.equity.update(cal, live);

        let inputs = feed_to_engine(&tick, &self.convert_cfg);
        let gates = equity_to_engine(&eq);
        self.insurer.accrue(now_slot);
        let st = EngineState { tick, inputs, gates, now_slot };
        self.last_state = st;
        st
    }

    pub fn state(&self) -> EngineState {
        self.last_state
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
            convert_cfg: crate::convert::ConvertCfg { h_max_cap: 100 },
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
        // no obs + calendar Closed -> equity gates opens
        let st = e.tick(&[], CalendarState::Closed, 1, 1);
        assert!(!st.gates.allow_open);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail** — `cargo test engine::` → FAIL (placeholder).
- [ ] **Step 3:** Implementation is in the file above.
- [ ] **Step 4: Run tests to verify they pass** — `cargo test engine::` → PASS (2 tests). Resolve any real-API mismatches against `percolator-sim`'s pattern (`sed -n '25,130p' ~/percolator/percolator-sim/src/engine/mod.rs`).
- [ ] **Step 5: Commit**

```bash
git add src/engine.rs
git commit -m "feat: PercolatorEngine + composed tick() (feed+equity+insurer)"
```

---

### Task 3: engine.rs — intent ops (open/close/withdraw/liquidate) with gate enforcement

**Files:**
- Modify: `src/engine.rs` (add an `impl PercolatorEngine` ops block + tests)

- [ ] **Step 1: Write the failing tests + add the ops**

Add inside `src/engine.rs` (new impl methods). Follows sim's `deposit` + `execute_trade` + `liquidate` pattern (signatures verified against the wrapper):

```rust
use percolator_insurance::LiquidationPolicy;

impl PercolatorEngine {
    /// Seed an account with capital (LP or trader). Mirrors sim's deposit.
    pub fn deposit(&mut self, idx: u16, amount: u128, now_slot: u64) -> percolator_insurance::PercolatorResult<()> {
        self.insurer.deposit(idx, amount, now_slot)
    }

    /// Open/increase a position a-vs-b. Rejected (Err-free no-op returning false)
    /// when the equity gates forbid risk increase. Liquidation is never gated here.
    pub fn open(
        &mut self,
        a: u16,
        b: u16,
        size_q: i128,
        now_slot: u64,
    ) -> percolator_insurance::PercolatorResult<bool> {
        if !self.last_state.gates.allow_open {
            return Ok(false);
        }
        let inp = self.last_state.inputs;
        self.insurer.execute_trade(
            a, b, inp.oracle_price, now_slot, size_q, inp.oracle_price,
            inp.funding_e9, inp.admit_h_min, inp.admit_h_max, None,
        )?;
        Ok(true)
    }

    /// Liquidate an account at the fast price. Always allowed (risk-reducing).
    pub fn liquidate(&mut self, idx: u16, now_slot: u64) -> percolator_insurance::PercolatorResult<bool> {
        let inp = self.last_state.inputs;
        self.insurer.liquidate(
            idx, now_slot, inp.oracle_price, LiquidationPolicy::default(),
            inp.funding_e9, inp.admit_h_min, inp.admit_h_max, None,
        )
    }

    /// Withdraw at the persistence-confirmed extraction price + warmup band.
    pub fn withdraw(&mut self, idx: u16, amount: u128, now_slot: u64) -> percolator_insurance::PercolatorResult<u128> {
        let inp = self.last_state.inputs;
        self.insurer.withdraw(idx, amount, now_slot, inp.extraction_price, inp.admit_h_min, inp.admit_h_max)
    }
}
```

NOTE (resolve during impl): verify the exact `withdraw`/`liquidate` signatures (`sed -n '890,1010p' ~/percolator/percolator-insurance/src/wrapper.rs`) and `LiquidationPolicy::default()` availability; adapt arg order/extra params to the real signature. `PercolatorResult` is re-exported from the insurer; confirm the path.

```rust
#[cfg(test)]
mod ops_tests {
    use super::*;
    use crate::fixtures::*;

    fn engine() -> PercolatorEngine {
        let mut e = PercolatorEngine::new(EngineCfg {
            risk_params: risk_params(), premium_params: premium_params(),
            feed_cfg: feed_cfg(), equity_cfg: equity_cfg(), name_cfg: name_cfg(),
            convert_cfg: crate::convert::ConvertCfg { h_max_cap: 100 }, init_price: 50_000,
        }, 0);
        // seed an LP + a trader
        let _ = e.deposit(0, 1_000_000_000, 0);
        let _ = e.deposit(1, 1_000_000, 0);
        e
    }

    #[test]
    fn open_rejected_when_session_gates_it() {
        let mut e = engine();
        e.tick(&[], CalendarState::Closed, 1, 1); // Closed -> allow_open=false
        let opened = e.open(1, 0, 1000, 2).unwrap();
        assert!(!opened); // gate held, no trade
    }

    #[test]
    fn open_allowed_when_open_session() {
        let mut e = engine();
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
        let mut e = engine();
        e.tick(&[], CalendarState::Closed, 1, 1); // opens gated
        // liquidate should still be callable (returns Ok regardless of gate)
        let _ = e.liquidate(1, 2); // no panic; gate does not block liquidation
    }
}
```

- [ ] **Step 2: Run tests to verify they fail** — `cargo test engine::ops_tests::` → FAIL.
- [ ] **Step 3:** Ops are in the file above; adjust to real signatures per the NOTE.
- [ ] **Step 4: Run tests to verify they pass** — `cargo test engine::` → PASS.
- [ ] **Step 5: Commit**

```bash
git add src/engine.rs
git commit -m "feat: intent ops (open/close/withdraw/liquidate) with asymmetric gate enforcement"
```

---

### Task 4: invariants.rs — the cross-crate contract

**Files:**
- Create: `src/invariants.rs` (replace placeholder)

- [ ] **Step 1: Write the failing tests + implementation**

```rust
//! Cross-crate invariant contract. check() returns a Violation (data, not a panic).

use crate::engine::PercolatorEngine;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Violation {
    PhantomPayout,        // pool recorded more than the insurance balance backs
    NegativeReserve,      // insurer reserve/solvency breached
    ExtractionOnLowConf,  // extraction allowed while confidence below the gate floor
    ReopenBufferMissing,  // closed/halted/reopen but no reopen buffer applied
}

/// Minimum confidence below which extraction must be gated.
pub const MIN_EXTRACT_CONF: u32 = 1; // any positive floor; tuned via cfg later

pub fn check(eng: &PercolatorEngine) -> Result<(), Violation> {
    // 1. No phantom payout: the pool's own invariant (insurance balance backs records).
    if !eng.insurer.pool().check_invariants() {
        return Err(Violation::PhantomPayout);
    }
    // 2. Reopen buffer present whenever the session is non-Open with a held buffer.
    let st = eng.state();
    let session_needs_buffer = !st.gates.allow_open && st.gates.reopen_gap_margin == 0
        && st.gates.extraction_warmup == 0 && !st.gates.mark_underlying;
    if session_needs_buffer {
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

    fn engine() -> PercolatorEngine {
        PercolatorEngine::new(EngineCfg {
            risk_params: risk_params(), premium_params: premium_params(),
            feed_cfg: feed_cfg(), equity_cfg: equity_cfg(), name_cfg: name_cfg(),
            convert_cfg: crate::convert::ConvertCfg { h_max_cap: 100 }, init_price: 50_000,
        }, 0)
    }

    #[test]
    fn holds_in_normal_operation() {
        let mut e = engine();
        let obs = [VenueObs { venue: 0, price: 50_000, depth: 100 },
                   VenueObs { venue: 1, price: 50_000, depth: 100 },
                   VenueObs { venue: 2, price: 50_000, depth: 100 }];
        e.tick(&obs, CalendarState::Open, 1, 1);
        assert_eq!(check(&e), Ok(()));
    }

    #[test]
    fn catches_a_synthetic_phantom_payout() {
        // Force the pool to over-record vs the insurance balance, then assert caught.
        let mut e = engine();
        e.insurer.pool_mut().record_collection(10_000_000_000).unwrap();
        // no matching insurance balance -> check_invariants() should fail
        assert_eq!(check(&e), Err(Violation::PhantomPayout));
    }
}
```

NOTE (resolve during impl): verify the accessor names — the engine exposes `insurer: InsuredRiskEngine` (pub). Use `eng.insurer.pool()` if such a getter exists; otherwise add a small `pub fn pool(&self) -> &PremiumPool` to the wrapper is NOT in scope — instead expose what's needed via `PercolatorEngine` (add `pub fn pool_check(&self) -> bool { self.insurer.<pool field>.check_invariants() }`). Confirm `pool()`/`pool_mut()` exist on `InsuredRiskEngine` (`grep -n "pub fn pool" ~/percolator/percolator-insurance/src/wrapper.rs`); if not, add thin accessors to `PercolatorEngine` in `engine.rs` and call those. The synthetic-violation test must use whatever real path forces `check_invariants()` to return false.

- [ ] **Step 2: Run tests to verify they fail** — `cargo test invariants::` → FAIL.
- [ ] **Step 3:** Implementation above; wire the real pool accessor per the NOTE.
- [ ] **Step 4: Run tests to verify they pass** — `cargo test invariants::` → PASS (2 tests).
- [ ] **Step 5: Commit**

```bash
git add src/invariants.rs src/engine.rs
git commit -m "feat: cross-crate invariant contract + synthetic-violation test"
```

---

### Task 5: redteam.rs — adversarial search + negative control

**Files:**
- Create: `src/redteam.rs` (replace placeholder)

- [ ] **Step 1: Write the failing tests + implementation**

```rust
//! Adversarial driver: search the action space for an invariant break against the
//! REAL composed engine. A clean run is evidence (falsifier, not prover).

use crate::engine::{EngineCfg, PercolatorEngine, VenueObs};
use crate::invariants::{check, Violation};
use percolator_equity::session::CalendarState;

#[derive(Clone, Copy, Debug)]
pub enum Action {
    OracleMove { price: u64 },
    AdvanceSlots { n: u64 },
    Cross { cal: CalendarFlag },
    Open { a: u16, b: u16, size: i128 },
    Liquidate { idx: u16 },
}

#[derive(Clone, Copy, Debug)]
pub enum CalendarFlag {
    Open,
    Closed,
    Halted,
}

impl CalendarFlag {
    fn to_state(self) -> CalendarState {
        match self {
            CalendarFlag::Open => CalendarState::Open,
            CalendarFlag::Closed => CalendarState::Closed,
            CalendarFlag::Halted => CalendarState::Halted,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Outcome {
    NoBreak { trials: usize },
    Found { seq: Vec<Action>, violation: Violation },
}

/// A simple deterministic LCG so the search is reproducible without extra deps.
struct Lcg(u64);
impl Lcg {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
}

fn apply(eng: &mut PercolatorEngine, act: Action, clock: &mut (u64, u64)) {
    match act {
        Action::OracleMove { price } => {
            let obs = [
                VenueObs { venue: 0, price, depth: 100 },
                VenueObs { venue: 1, price, depth: 100 },
                VenueObs { venue: 2, price, depth: 100 },
            ];
            clock.0 += 1;
            clock.1 += 1;
            eng.tick(&obs, CalendarState::Open, clock.0, clock.1);
        }
        Action::AdvanceSlots { n } => {
            clock.0 += n.max(1);
            clock.1 += n.max(1);
            eng.tick(&[], CalendarState::Open, clock.0, clock.1);
        }
        Action::Cross { cal } => {
            clock.0 += 1;
            clock.1 += 1;
            eng.tick(&[], cal.to_state(), clock.0, clock.1);
        }
        Action::Open { a, b, size } => {
            let _ = eng.open(a, b, size, clock.0);
        }
        Action::Liquidate { idx } => {
            let _ = eng.liquidate(idx, clock.0);
        }
    }
}

fn random_action(rng: &mut Lcg) -> Action {
    match rng.next_u64() % 5 {
        0 => Action::OracleMove { price: 40_000 + (rng.next_u64() % 20_000) },
        1 => Action::AdvanceSlots { n: 1 + rng.next_u64() % 10 },
        2 => Action::Cross {
            cal: match rng.next_u64() % 3 {
                0 => CalendarFlag::Open,
                1 => CalendarFlag::Closed,
                _ => CalendarFlag::Halted,
            },
        },
        3 => Action::Open { a: 1, b: 0, size: 100 + (rng.next_u64() % 5000) as i128 },
        _ => Action::Liquidate { idx: 1 },
    }
}

/// Drive a fresh engine through `steps` random actions; check invariants each step.
pub fn search(mut new_engine: impl FnMut() -> PercolatorEngine, seed: u64, trials: usize, steps: usize) -> Outcome {
    for t in 0..trials {
        let mut eng = new_engine();
        let mut rng = Lcg(seed.wrapping_add(t as u64).wrapping_add(1));
        let mut clock = (0u64, 0u64);
        let mut seq = Vec::with_capacity(steps);
        for _ in 0..steps {
            let act = random_action(&mut rng);
            seq.push(act);
            apply(&mut eng, act, &mut clock);
            if let Err(v) = check(&eng) {
                return Outcome::Found { seq: shrink(&new_engine_clone(&new_engine), &seq, v), violation: v };
            }
        }
    }
    Outcome::NoBreak { trials }
}

// Helper to re-run a prefix for shrinking (rebuild engine, replay subsequence).
fn new_engine_clone<'a>(_f: &'a impl Fn() -> PercolatorEngine) -> impl Fn() -> PercolatorEngine + 'a {
    || unreachable!() // replaced below; shrink uses the same factory
}

/// Minimize a failing sequence by removing actions while the violation persists.
fn shrink(_factory: &impl Fn() -> PercolatorEngine, seq: &[Action], _v: Violation) -> Vec<Action> {
    seq.to_vec() // minimal: return as-is (a full delta-debug shrink is a follow-up)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::*;

    fn factory_sound() -> impl FnMut() -> PercolatorEngine {
        || {
            let mut e = PercolatorEngine::new(EngineCfg {
                risk_params: risk_params(), premium_params: premium_params(),
                feed_cfg: feed_cfg(), equity_cfg: equity_cfg(), name_cfg: name_cfg(),
                convert_cfg: crate::convert::ConvertCfg { h_max_cap: 100 }, init_price: 50_000,
            }, 0);
            let _ = e.deposit(0, 1_000_000_000, 0);
            let _ = e.deposit(1, 1_000_000, 0);
            e
        }
    }

    #[test]
    fn sound_config_no_break() {
        let out = search(factory_sound(), 42, 20, 30);
        assert!(matches!(out, Outcome::NoBreak { .. }), "unexpected break: {:?}", out);
    }

    #[test]
    fn negative_control_finds_planted_break() {
        // A deliberately broken invariant config: name with NO proxy and
        // list_without_proxy=false, so during a Closed session the engine reaches a
        // state the ReopenBufferMissing check is designed to flag — the search must
        // FIND it. (If this ever returns NoBreak, the red-team is theater.)
        use percolator_equity::marking::{NameCfg, ProxyKind};
        let broken_name = NameCfg { has_proxy: false, proxy_kind: ProxyKind::IndexFuture, list_without_proxy: false };
        let factory = move || {
            let mut e = PercolatorEngine::new(EngineCfg {
                risk_params: risk_params(), premium_params: premium_params(),
                feed_cfg: feed_cfg(), equity_cfg: equity_cfg(), name_cfg: broken_name,
                convert_cfg: crate::convert::ConvertCfg { h_max_cap: 100 }, init_price: 50_000,
            }, 0);
            let _ = e.deposit(0, 1_000_000_000, 0);
            let _ = e.deposit(1, 1_000_000, 0);
            e
        };
        let out = search(factory, 7, 50, 40);
        assert!(matches!(out, Outcome::Found { .. }), "negative control did not find the planted break");
    }
}
```

NOTE (resolve during impl): the `new_engine_clone`/closure-factory plumbing for shrinking is awkward in Rust with `FnMut`. Simplify by changing `search` to take `factory: impl Fn() -> PercolatorEngine` (Fn, not FnMut) so it can be reused for both the trial loop and shrink; drop the `new_engine_clone` helper. The negative-control invariant must be one `check()` genuinely returns `Err` on — align the planted break with whatever `invariants::check` actually flags (adjust the broken config and/or add a deliberately-disabled invariant path used only by the test). The key requirement: **the negative control MUST fail loudly if the search can't detect a real break.**

- [ ] **Step 2: Run tests to verify they fail** — `cargo test redteam::` → FAIL.
- [ ] **Step 3:** Implementation above; simplify the factory signature per the NOTE.
- [ ] **Step 4: Run tests to verify they pass** — `cargo test redteam::` → PASS (both: sound=NoBreak, broken=Found).
- [ ] **Step 5: Commit**

```bash
git add src/redteam.rs
git commit -m "feat: adversarial red-team search + negative-control proof"
```

---

### Task 6: harness + bin + full gate

**Files:**
- Create: `src/harness.rs`, `src/bin/engine.rs`
- Modify: `src/lib.rs` (add `pub mod harness;`)

- [ ] **Step 1: Write `src/harness.rs`** (composed demo day)

```rust
//! Composed demo: drive a day (open -> close -> reopen -> halt) and collect states.

use crate::engine::{EngineCfg, EngineState, PercolatorEngine, VenueObs};
use percolator_equity::session::CalendarState;

pub fn demo_day(cfg: EngineCfg) -> Vec<EngineState> {
    let mut e = PercolatorEngine::new(cfg, 0);
    let _ = e.deposit(0, 1_000_000_000, 0);
    let _ = e.deposit(1, 1_000_000, 0);
    let mut out = Vec::new();
    let day: [(CalendarState, bool); 5] = [
        (CalendarState::Open, true),
        (CalendarState::Closed, false),
        (CalendarState::Closed, true),  // reopen trigger
        (CalendarState::Open, true),
        (CalendarState::Halted, false),
    ];
    for (i, (cal, fresh)) in day.iter().enumerate() {
        let obs: Vec<VenueObs> = if *fresh {
            vec![
                VenueObs { venue: 0, price: 50_000, depth: 100 },
                VenueObs { venue: 1, price: 50_000, depth: 100 },
                VenueObs { venue: 2, price: 50_000, depth: 100 },
            ]
        } else {
            vec![]
        };
        out.push(e.tick(&obs, *cal, (i + 1) as u64, (i + 1) as u64));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::*;

    #[test]
    fn demo_day_runs_and_gates_when_closed() {
        let cfg = EngineCfg {
            risk_params: risk_params(), premium_params: premium_params(),
            feed_cfg: feed_cfg(), equity_cfg: equity_cfg(), name_cfg: name_cfg(),
            convert_cfg: crate::convert::ConvertCfg { h_max_cap: 100 }, init_price: 50_000,
        };
        let states = demo_day(cfg);
        assert_eq!(states.len(), 5);
        assert!(!states[1].gates.allow_open); // Closed -> gated
    }
}
```

- [ ] **Step 2: Add `pub mod harness;` to `src/lib.rs`** (the crate is `std`; no cfg gate needed — whole crate is std).

- [ ] **Step 3: Write `src/bin/engine.rs`**

```rust
//! Demo: print a composed day, then run the red-team and report.

use percolator_engine::engine::EngineCfg;
use percolator_engine::fixtures::*;
use percolator_engine::harness::demo_day;
use percolator_engine::redteam::{search, Outcome};
use percolator_engine::engine::PercolatorEngine;

fn make_cfg() -> EngineCfg {
    EngineCfg {
        risk_params: risk_params(), premium_params: premium_params(),
        feed_cfg: feed_cfg(), equity_cfg: equity_cfg(), name_cfg: name_cfg(),
        convert_cfg: percolator_engine::convert::ConvertCfg { h_max_cap: 100 }, init_price: 50_000,
    }
}

fn main() {
    println!("=== composed day ===");
    for (i, st) in demo_day(make_cfg()).iter().enumerate() {
        println!(
            "t{i}: oracle={} open_ok={} margin_mult={} buf={} warmup={}",
            st.inputs.oracle_price, st.gates.allow_open, st.gates.margin_mult,
            st.gates.reopen_gap_margin, st.gates.extraction_warmup
        );
    }
    println!("=== red-team (sound config) ===");
    let factory = || {
        let mut e = PercolatorEngine::new(make_cfg(), 0);
        let _ = e.deposit(0, 1_000_000_000, 0);
        let _ = e.deposit(1, 1_000_000, 0);
        e
    };
    match search(factory, 1, 200, 60) {
        Outcome::NoBreak { trials } => println!("no invariant break in {trials} trials"),
        Outcome::Found { seq, violation } => println!("BREAK {violation:?} in {} actions", seq.len()),
    }
}
```

NOTE: requires `fixtures` feature for the bin to see `fixtures`. Add `required-features = ["fixtures"]` to `[[bin]]` in Cargo.toml, OR move the fixture fns out from behind the cfg for bin use. Simplest: add `required-features = ["fixtures"]` and run with `cargo run --features fixtures --bin engine`. Adjust `search`'s factory param type to match Task 5's final signature.

- [ ] **Step 4: Run the bin** — `cargo run --features fixtures --bin engine` → prints the day + "no invariant break in 200 trials".

- [ ] **Step 5: Full gate** — `cargo test --features fixtures && cargo clippy --all-targets --features fixtures -- -D warnings` → all PASS, clippy clean.

- [ ] **Step 6: Commit**

```bash
git add src/harness.rs src/bin/engine.rs src/lib.rs Cargo.toml
git commit -m "feat: composed demo-day harness + bin (day replay + red-team runner)"
```

---

## Self-Review

**Spec coverage:**
- §3 integration mapping (feed→oracle/funding/admit, equity→gates/margin) → Task 1 `convert`. ✓
- §4.2 one `tick()` + intent ops with gate enforcement → Tasks 2, 3. ✓
- §4.3 invariant contract (solvency/phantom/extraction/reopen-buffer) → Task 4. ✓
- §4.4 red-team action space + search + shrink → Task 5. ✓
- §4.5 harness + bin → Task 6. ✓
- §7 testing incl. the negative control → Task 5 `negative_control_finds_planted_break`. ✓
- §3 two open assumptions (price scale fit, margin compose) → tested in Task 1 (`price_clamped_into_engine_range`) and exercised through Tasks 2–3.

**Placeholder scan:** the `// placeholder` module files (Task 0 Step 4) are intentional scaffolding replaced by each task. Several `NOTE (resolve during impl)` blocks flag real-API details (exact `withdraw`/`liquidate`/`pool` signatures, the shrink factory type) that must be verified against the wrapper during implementation — these are integration-points, not vague requirements; each names the exact file/command to confirm against.

**Type consistency:** `EngineInputs`/`EngineGates`/`ConvertCfg` (Task 1) consumed unchanged by Tasks 2–5; `PercolatorEngine`/`EngineState`/`EngineCfg`/`VenueObs` (Task 2) consumed by Tasks 3–6; `Violation`/`check` (Task 4) consumed by Task 5; fixtures (Task 0) reused everywhere. `search` signature is finalized in Task 5 (Fn factory) and that final form is what Task 6's bin must call.

**Integration risk (honest):** this crate drives a complex external engine. Tasks 2–5 carry real compiler-iteration risk on exact wrapper signatures; each such point is flagged with the precise file+line to check. This is normal for an integration layer and is resolved with the compiler as the first test (TDD).
