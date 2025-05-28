//// filepath: /home/starezax/Desktop/liquidator_arsen/src/kamino.rs
use solana_sdk::pubkey::Pubkey;
use borsh::{BorshDeserialize, BorshSerialize};

#[derive(BorshDeserialize, BorshSerialize, Debug)]
pub struct LastUpdate {
    pub slot: u64,
    pub stale: u8,
    pub price_status: u8,
    pub placeholder: [u8; 6],
}

impl Default for LastUpdate {
    fn default() -> Self {
        Self {
            slot: 0,
            stale: 0,
            price_status: 0,
            placeholder: [0; 6],
        }
    }
}

#[derive(BorshDeserialize, BorshSerialize, Debug)]
pub struct ObligationCollateral {
    pub deposit_reserve: Pubkey,
    pub deposited_amount: u64,
    pub market_value_sf: u128,
    pub borrowed_amount_against_this_collateral_in_elevation_group: u64,
    pub padding: [u64; 9],
}

#[derive(BorshDeserialize, BorshSerialize, Debug)]
pub struct BigFractionBytes {
    pub value: [u64; 4],
    pub padding: [u64; 2],
}

#[derive(BorshDeserialize, BorshSerialize, Debug)]
pub struct ObligationLiquidity {
    pub borrow_reserve: Pubkey,
    pub cumulative_borrow_rate_bsf: BigFractionBytes,
    pub padding: u64,
    pub borrowed_amount_sf: u128,
    pub market_value_sf: u128,
    pub borrow_factor_adjusted_market_value_sf: u128,
    pub borrowed_amount_outside_elevation_groups: u64,
    pub padding2: [u64; 7],
}

#[derive(BorshDeserialize, BorshSerialize, Debug, Default)]
pub struct ObligationOrder {
    pub condition_threshold_sf: u128,
    pub opportunity_parameter_sf: u128,
    pub min_execution_bonus_bps: u16,
    pub max_execution_bonus_bps: u16,
    pub condition_type: u8,
    pub opportunity_type: u8,
    pub padding1: [u8; 10],
    pub padding2: [u128; 5],
}

#[derive(BorshDeserialize, BorshSerialize, Debug)]
pub struct Obligation {
    pub tag: u64,
    pub last_update: LastUpdate,
    pub lending_market: Pubkey,
    pub owner: Pubkey,
    pub deposits: [ObligationCollateral; 8],
    pub lowest_reserve_deposit_liquidation_ltv: u64,
    pub deposited_value_sf: u128,
    pub borrows: [ObligationLiquidity; 5],
    pub borrow_factor_adjusted_debt_value_sf: u128,
    pub borrowed_assets_market_value_sf: u128,
    pub allowed_borrow_value_sf: u128,
    pub unhealthy_borrow_value_sf: u128,
    pub deposits_asset_tiers: [u8; 8],
    pub borrows_asset_tiers: [u8; 5],
    pub elevation_group: u8,
    pub num_of_obsolete_deposit_reserves: u8,
    pub has_debt: u8,
    pub referrer: Pubkey,
    pub borrowing_disabled: u8,
    pub autodeleverage_target_ltv_pct: u8,
    pub lowest_reserve_deposit_max_ltv_pct: u8,
    pub num_of_obsolete_borrow_reserves: u8,
    pub reserved: [u8; 4],
    pub highest_borrow_factor_pct: u64,
    pub autodeleverage_margin_call_started_timestamp: u64,
    pub orders: [ObligationOrder; 2],
    pub padding_3: [u64; 93],
}