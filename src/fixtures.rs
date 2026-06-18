//! Canonical test/demo configs. Built once, reused by tests, harness, bin, red-team.
#![cfg(any(test, feature = "fixtures"))]

use percolator::{RiskParams, U128};
use percolator_equity::equity::EquityCfg;
use percolator_equity::fixed::{CONF, SCALE};
use percolator_equity::marking::{MarkCfg, NameCfg, ProxyKind};
use percolator_equity::riskmode::RiskCfg;
use percolator_equity::session::SessionCfg;
use percolator_equity::surcharge::SurchargeCfg;
use percolator_feed::FeedCfg;
use percolator_insurance::{PremiumParams, MULT_SCALE};

pub fn risk_params() -> RiskParams {
    RiskParams {
        maintenance_margin_bps: 500,
        initial_margin_bps: 1000,
        trading_fee_bps: 10,
        max_accounts: 64,
        liquidation_fee_bps: 100,
        liquidation_fee_cap: U128::new(1_000_000_000),
        min_liquidation_abs: U128::new(0),
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
