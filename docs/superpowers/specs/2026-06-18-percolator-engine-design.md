# percolator-engine — Design Spec

**Date:** 2026-06-18
**Status:** Approved direction, pre-implementation
**Location:** `~/percolator-engine/` (separate crate/repo). The **integration layer**
that composes the suite — and the home of the adversarial red-team (originally
scoped as standalone sub-project ③, folded in here as the verification module).

## 1. Problem & goal

The suite is four islands. `percolator-insurance` (the `InsuredRiskEngine` wrapper)
is the only crate wired into a running engine. `percolator-feed` emits `FeedTick`
and `percolator-equity` emits `EquityRiskState` — but **nothing consumes them**
(verified: no crate references `percolator_feed`/`percolator_equity`). Every spec
deferred the wiring to "the integrator," and the integrator was never built. So the
cross-crate interaction — the only thing not already unit-tested in the leaf crates
— does not exist as code.

**Goal:** build the integrator. Compose `FeedTick` → oracle/funding and
`EquityRiskState` → margin/admission/extraction into the real `InsuredRiskEngine`
behind **one `tick()`**, reconcile the units, enforce a cross-crate **invariant
contract**, and red-team the *real* composition (not a fiction) for invariant
breaks. This turns four islands into one usable engine and delivers ③'s value on a
real target.

## 2. Why this and not a standalone red-team (roast-driven)

A standalone red-team ③ would have to construct the composition *inside itself* to
have anything to attack — marking its own homework, and checking invariants
(no-phantom-payout, funding cap, bounded A·K·F, PML solvency) that the leaf crates
and the fork already test individually. The genuinely-untested surface is the
*composition*, which doesn't exist yet. So the necessary artifact is the integration
layer; the red-team becomes its verification once a real `tick()` exists.

## 3. The integration mapping (grounded in the real API)

`InsuredRiskEngine` (real signatures): `execute_trade(a, b, oracle_price: u64,
now_slot, size_q: i128, exec_price: u64, funding_rate_e9: i128, admit_h_min: u64,
admit_h_max: u64, admit_h_max_consumption_threshold_bps_opt: Option<u128>)`;
`liquidate(idx, now_slot, oracle_price: u64, policy, funding_rate_e9, admit_h_min,
admit_h_max, …)`; `withdraw(…)`; `accrue(now_slot)`; `collect_accrued_premium(idx,
now_slot)`; `reconcile_pool()`.

| Engine input | Source | Reconciliation |
|---|---|---|
| `oracle_price` (liquidate/mark) | `FeedTick.fast_price` | scale-convert feed `SCALE=1e9` → engine price (`MAX_ORACLE_PRICE`); clamp |
| withdraw/realize price | `FeedTick.extraction_price` | same convert; the persistence-confirmed price |
| `funding_rate_e9` | `FeedTick.funding_e9` | **identity** (both `i128` e9) |
| `admit_h_min/max`, consumption threshold | `FeedTick.extraction_gate` + equity `extraction_warmup` | map confidence/warmup → the H-haircut admission band (lower confidence ⇒ wider warmup) |
| risk-increase admission | equity `allow_risk_increase` ∧ `admission == Admissible` | gate `open` (risk-increasing `execute_trade`); never gate `liquidate`/`close` |
| maintenance margin | equity `margin_mult`, `reopen_gap_margin` | fold into the engine's `RiskParams` margin for the op |
| mark source / trust | equity `mark`, `mark_conf` | choose underlying vs proxy price; `mark_conf` feeds the admit band |

The two open assumptions this layer must *test*, not assume: (a) the feed price
scale fits the engine oracle range; (b) `margin_mult`/`reopen_gap_margin` compose
with the insurer margin/`volatility_mult` model without double-counting.

## 4. Architecture (`std` integration tier)

`percolator-engine` is a `std` crate (the application/integration tier, like
`percolator-sim`). The leaf crates keep their `no_std` discipline; composition logic
stays integer/deterministic, the red-team uses `std` (alloc + search). Depends on
`percolator-insurance`, `percolator-feed`, `percolator-equity` (path-deps locally;
pinned git-deps + committed `Cargo.lock` for publish — the calibration pattern).

### Modules

1. **`convert.rs`** — pure adapters. `feed_to_engine(&FeedTick, &ConvertCfg) ->
   EngineInputs { oracle_price, extraction_price, funding_e9, admit_h_min,
   admit_h_max, consumption_threshold }`; `equity_to_engine(&EquityRiskState,
   &ConvertCfg) -> EngineGates { allow_open, margin_mult, reopen_gap_margin,
   extraction_warmup, mark_price_source }`. Scale conversion + clamps live here;
   no engine state touched (unit-testable in isolation).
2. **`engine.rs`** — `PercolatorEngine { insurer: InsuredRiskEngine, feed: RiskFeed,
   equity: EquityEngine, cfg }`. `tick(obs: &[VenueObs], cal: CalendarState,
   now_slot) -> EngineState` advances feed + equity, computes `EngineInputs` +
   `EngineGates`, caches them, and `accrue`s the insurer. Intent ops:
   `open(a, b, size_q, exec_price)`, `close(...)`, `withdraw(idx, amount)`,
   `liquidate(idx)`. `open` is rejected when `!gates.allow_open`; `withdraw` uses
   `extraction_price` + the warmup band; `liquidate` uses `fast_price` and is never
   gated. `EngineState` surfaces the composed view (session, mark, gates, pool
   health) for callers and the red-team.
3. **`invariants.rs`** — the contract. `check(&PercolatorEngine) -> Result<(),
   Violation>` over: **solvency** (insurer balance reconciles to recorded pool
   consumption, no negative reserve), **no phantom payout** (`pool.check_invariants`),
   **bounded socialization** (haircut/A·K·F within configured bounds),
   **no-extraction-on-low-confidence** (a withdraw cannot realize on a sub-threshold
   confidence price), **reopen-buffer-applied** (during Closed/Halted/Reopen the
   maintenance margin includes the equity `reopen_gap_margin`). Each `Violation`
   names the breached invariant + the state.
4. **`redteam.rs`** (`std`) — the adversarial verification. `Action` enum
   (`OracleMove(bps)`, `FundingPush(e9)`, `Open{a,b,size}`, `Close{idx}`,
   `Liquidate{idx}`, `AdvanceSlots(n)`, `CrossSessionBoundary(CalendarState)`,
   `InjectReopenGap(bps)`). A `search(seed, cfg)` drives a fresh `PercolatorEngine`
   through random/greedy action sequences, calling `invariants::check` after each
   step; on a break it **shrinks** to a minimal failing sequence and returns
   `Found(Counterexample)`, else `NoBreak { trials }`.
5. **`harness.rs` + `bin/engine.rs`** (`std`) — a composed demo day (close→reopen,
   earnings, a manipulation attempt) printing the `EngineState` series; a red-team
   runner CLI.

## 5. Data flow

```
venue obs ─┐
calendar ──┼─ PercolatorEngine::tick ─→ feed → FeedTick ─┐
clock  ────┘                          equity → EquityRiskState ─┤
                                                                ▼
                              convert → EngineInputs + EngineGates
                                                                ▼
                  intent op (open/close/withdraw/liquidate) → InsuredRiskEngine
                                                                ▼
                                      invariants::check (tests + red-team)
```

## 6. Error handling

Engine ops return `Result` (propagating the insurer's `crate::Result`). `convert`
saturates/clamps (no panic on extreme prices). `invariants::check` returns
`Result<(), Violation>` (never panics — a violation is data, not a crash). The
red-team treats a panic in the engine as a `Violation` too (a panic IS a failure).

## 7. Testing (TDD)

- **convert:** feed price → oracle round-trips within `MAX_ORACLE_PRICE`; funding
  passthrough is identity; a low-confidence `FeedTick` widens the admit band; equity
  `allow_risk_increase=false` ⇒ `allow_open=false`; reopen buffer flows into margin.
- **engine:** a composed `tick` yields sane inputs; `open` rejected when equity gates
  it (Closed/PreEvent/ReduceOnly); `withdraw` uses the extraction price + warmup;
  `liquidate` runs even when opens are gated.
- **invariants:** each invariant holds in normal operation AND the checker **catches**
  a synthetic violation (e.g. a hand-forced pool over-record trips no-phantom-payout).
- **redteam — the negative control (critical):** the search finds **no** break on a
  sound config over N trials; and on a **deliberately broken** config (funding cap
  disabled / reopen buffer zeroed) the search **does** find and shrink a break. This
  proves the red-team can detect failure — without it, "no break found" is theater.

## 8. Out of scope (later)

The options-implied-move surcharge (equity follow-up); live venue/calendar feeds
(synthetic + the leaf crates' CSV now); the on-chain submission path; replacing
`percolator-sim`'s backtest/optimizer (different concern — sim optimizes params on
history; this composes the production risk stack). `percolator-sim` may later consume
`percolator-engine` in place of its toy oracle, but that refactor is not in scope.

## 9. Honest residuals

The invariant contract is only as complete as the invariants we enumerate — the
red-team can only find breaks of properties we wrote down (it does not discover
*new* properties). Composition correctness depends on the convert layer's
scale/margin mapping being right; §3's two assumptions are tested here but a real
venue integration could surface more. The red-team's coverage is bounded by its
action space and trial budget (it is a falsifier, not a prover) — a clean run is
evidence, not proof.
