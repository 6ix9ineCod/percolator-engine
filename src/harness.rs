//! Composed demo: drive a day (open -> close -> reopen -> halt) and collect states.

use crate::engine::{EngineCfg, EngineState, PercolatorEngine, VenueObs};
use percolator_equity::session::CalendarState;

pub fn demo_day(cfg: EngineCfg) -> Vec<EngineState> {
    let mut e = PercolatorEngine::new(cfg, 0);
    let _ = e.deposit(0, 1_000_000_000, 0);
    let _ = e.deposit(1, 1_000_000, 0);
    let day: [(CalendarState, bool); 5] = [
        (CalendarState::Open, true),
        (CalendarState::Closed, false),
        (CalendarState::Closed, true), // reopen trigger (fresh ticks return)
        (CalendarState::Open, true),
        (CalendarState::Halted, false),
    ];
    let mut out = Vec::new();
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
            risk_params: risk_params(),
            premium_params: premium_params(),
            feed_cfg: feed_cfg(),
            equity_cfg: equity_cfg(),
            name_cfg: name_cfg(),
            convert_cfg: crate::convert::ConvertCfg { h_max_cap: 100 },
            init_price: 50_000,
        };
        let states = demo_day(cfg);
        assert_eq!(states.len(), 5);
        assert!(!states[1].gates.allow_open); // Closed -> gated
    }
}
