//! Demo: print a composed day, then run the red-team and report.

use percolator_engine::convert::ConvertCfg;
use percolator_engine::engine::{EngineCfg, PercolatorEngine};
use percolator_engine::fixtures::*;
use percolator_engine::harness::demo_day;
use percolator_engine::redteam::{search, Outcome};

fn make_cfg() -> EngineCfg {
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

fn main() {
    println!("=== composed day ===");
    for (i, st) in demo_day(make_cfg()).iter().enumerate() {
        println!(
            "t{i}: slot={} | oracle={} open_ok={} mark_underlying={} margin_mult={} buf={} warmup={}",
            st.now_slot,
            st.inputs.oracle_price,
            st.gates.allow_open,
            st.gates.mark_underlying,
            st.gates.margin_mult,
            st.gates.reopen_gap_margin,
            st.gates.extraction_warmup,
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
        Outcome::Found { seq, violation } => {
            println!("BREAK {violation:?} in {} actions: {seq:?}", seq.len())
        }
    }
}
