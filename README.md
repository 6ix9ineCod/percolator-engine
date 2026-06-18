# percolator-engine

The **integration layer** that composes the Percolator perp risk suite into one
engine — and the home of the adversarial **red-team** that attacks the real
composition for invariant breaks.

## Why

The suite was four islands. `percolator-insurance` (the `InsuredRiskEngine`) is the
only crate wired into a running engine; `percolator-feed` emits `FeedTick` and
`percolator-equity` emits `EquityRiskState`, but nothing consumed them. Every spec
deferred the wiring to "the integrator" — who was never built. So the only
genuinely-untested surface, the *composition*, did not exist as code. This crate is
that integrator. (A standalone red-team was rejected: with no composition to attack,
it would have marked its own homework.)

## What it does

One `tick(observations, calendar, slot, ts)` advances the feed and the equity
session-state engine, derives the engine inputs, and feeds the real
`InsuredRiskEngine`:

| Engine input | Source |
|---|---|
| `oracle_price` (liquidation/mark) | feed `fast_price`, clamped to range |
| withdraw/realize price | feed `extraction_price` (persistence-confirmed) |
| `funding_rate_e9` | feed `funding_e9` (identity — units match) |
| admission band `[h_min, h_max]` | the engine's configured band (see Findings) |
| open admission, margin, mark | equity `allow_risk_increase ∧ Admissible`, `margin_mult`, `reopen_gap_margin`, `mark` |

Intent ops — `open` / `close` / `withdraw` / `liquidate` — route through the insurer
with the derived params and **enforce the equity gates asymmetrically**: naked
risk-increase is blocked in Closed/Halted/PreEvent (and for non-Admissible names);
liquidation and risk-reduction are never gated.

## The cross-crate invariant contract + red-team

`invariants::check` enforces the composition's contract — pool accounting solvency
(`paid_out ≤ collected`) and the **gate-protection** rule (a gated, proxy-marked
session must hold gap protection). `redteam::search` drives the *real* composed
engine through random adversarial action sequences (oracle moves, session crossings,
opens, liquidations, clock jumps), checking the invariants after every step and
**delta-debug shrinking** any break to a minimal counterexample.

A red-team that never finds anything is theater, so the suite includes a **negative
control**: against a deliberately mis-configured engine (gap protection zeroed) the
search *must* find and shrink the planted break. It does — which is what licenses
trusting its "no break found" on the sound config.

## Findings (the assumptions the spec said to test)

- **Funding units fit** the engine identically (both `i128` e9).
- **Price scale fits**: the feed's `1e9` prices sit inside the engine's
  `MAX_ORACLE_PRICE = 1e12` ceiling (clamped).
- **Admission band**: the warmup band must be the engine's configured `[0, h_max]`.
  A confidence-derived degenerate `h_max = 0` triggers an internal engine overflow,
  so confidence-modulated warmup is a documented follow-up — the confidence signal
  still reaches the engine via equity's `extraction_warmup` and `mark_conf`.

## Usage

```
cargo test --features fixtures                       # 16 tests
cargo run --features fixtures --bin engine           # composed day + red-team run
cargo clippy --all-targets --features fixtures -- -D warnings
```

Depends on `percolator`, `percolator-insurance`, `percolator-feed`,
`percolator-equity` (path-deps). Design + plan:
`docs/superpowers/{specs,plans}/2026-06-18-*`.

## Scope

Produces/validates the composition; it does not replace `percolator-sim` (a
backtest/optimizer — a different concern). Live venue/calendar feeds, the
options-implied-move surcharge, and confidence-modulated warmup are follow-ups.
