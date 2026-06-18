//! Adversarial driver: search the action space for an invariant break against the
//! REAL composed engine. A clean run is evidence (a falsifier, not a prover).

use crate::engine::{PercolatorEngine, VenueObs};
use crate::invariants::{check, Violation};
use percolator_equity::session::CalendarState;

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

#[derive(Clone, Copy, Debug)]
pub enum Action {
    OracleMove { price: u64 },
    AdvanceSlots { n: u64 },
    Cross { cal: CalendarFlag },
    Open { a: u16, b: u16, size: i128 },
    Liquidate { idx: u16 },
}

#[derive(Clone, Debug)]
pub enum Outcome {
    NoBreak { trials: usize },
    Found { seq: Vec<Action>, violation: Violation },
}

/// Deterministic LCG so the search is reproducible without extra deps.
struct Lcg(u64);
impl Lcg {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
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

/// `clock` is (slot, ts), advanced by each action.
fn apply(eng: &mut PercolatorEngine, act: Action, clock: &mut (u64, u64)) {
    match act {
        Action::OracleMove { price } => {
            clock.0 += 1;
            clock.1 += 1;
            let obs = [
                VenueObs { venue: 0, price, depth: 100 },
                VenueObs { venue: 1, price, depth: 100 },
                VenueObs { venue: 2, price, depth: 100 },
            ];
            eng.tick(&obs, CalendarState::Open, clock.0, clock.1);
        }
        Action::AdvanceSlots { n } => {
            let step = n.max(1);
            clock.0 += step;
            clock.1 += step;
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

/// Replay a sequence on a fresh engine; return the first invariant violation.
fn replay_breaks(factory: &impl Fn() -> PercolatorEngine, seq: &[Action]) -> Option<Violation> {
    let mut eng = factory();
    let mut clock = (0u64, 0u64);
    for &act in seq {
        apply(&mut eng, act, &mut clock);
        if let Err(v) = check(&eng) {
            return Some(v);
        }
    }
    None
}

/// Delta-debug: greedily drop actions while the SAME violation persists.
fn shrink(factory: &impl Fn() -> PercolatorEngine, seq: &[Action], v: Violation) -> Vec<Action> {
    let mut cur = seq.to_vec();
    let mut i = 0;
    while i < cur.len() {
        let mut trial = cur.clone();
        trial.remove(i);
        if replay_breaks(factory, &trial) == Some(v) {
            cur = trial; // shorter sequence still breaks the same way
        } else {
            i += 1;
        }
    }
    cur
}

/// Drive fresh engines through random action sequences; check invariants after
/// every action. Returns a minimal counterexample on the first break, else NoBreak.
pub fn search(
    factory: impl Fn() -> PercolatorEngine,
    seed: u64,
    trials: usize,
    steps: usize,
) -> Outcome {
    for t in 0..trials {
        let mut rng = Lcg(seed.wrapping_add(t as u64).wrapping_add(1));
        let seq: Vec<Action> = (0..steps).map(|_| random_action(&mut rng)).collect();
        if let Some(v) = replay_breaks(&factory, &seq) {
            return Outcome::Found { seq: shrink(&factory, &seq, v), violation: v };
        }
    }
    Outcome::NoBreak { trials }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::EngineCfg;
    use crate::fixtures::*;

    fn sound_cfg() -> EngineCfg {
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

    fn seeded(cfg: EngineCfg) -> PercolatorEngine {
        let mut e = PercolatorEngine::new(cfg, 0);
        let _ = e.deposit(0, 1_000_000_000, 0);
        let _ = e.deposit(1, 1_000_000, 0);
        e
    }

    #[test]
    fn sound_config_no_break() {
        let out = search(|| seeded(sound_cfg()), 42, 30, 40);
        assert!(matches!(out, Outcome::NoBreak { .. }), "unexpected break: {:?}", out);
    }

    #[test]
    fn negative_control_finds_planted_break() {
        // Deliberately broken config: zero the gap protection. A Closed/Halted
        // session then holds NO buffer -> ReopenBufferMissing. The search MUST find
        // it; if this ever returns NoBreak, the red-team is theater.
        let broken = || {
            let mut c = sound_cfg();
            c.equity_cfg.surcharge.reopen_gap_margin_closed = 0;
            c.equity_cfg.surcharge.reopen_gap_margin_halted = 0;
            c.equity_cfg.risk.halted_extraction_warmup = 0;
            seeded(c)
        };
        let out = search(broken, 7, 50, 40);
        match out {
            Outcome::Found { violation, seq } => {
                assert_eq!(violation, Violation::ReopenBufferMissing);
                assert!(!seq.is_empty());
            }
            Outcome::NoBreak { .. } => panic!("negative control did not find the planted break"),
        }
    }
}
