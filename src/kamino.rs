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

impl Obligation {
    pub fn get_reserve_addresses(&self) -> Vec<Pubkey> {
        let mut result = Vec::new();

        for deposit in &self.deposits {
            if deposit.deposit_reserve != Pubkey::default() {
                result.push(deposit.deposit_reserve);
            }
        }

        for borrow in &self.borrows {
            if borrow.borrow_reserve != Pubkey::default() {
                result.push(borrow.borrow_reserve);
            }
        }

        result.sort();
        result.dedup();
        result
    }
}

#[derive(Debug)]
pub struct ReserveData {
    pub mint_pubkey: Pubkey,
    pub decimals: u8,
    pub market_price: u128,
    pub oracle_pubkey: Pubkey,
    pub token_name: String,
}

impl ReserveData {
    pub fn try_parse_from_account_data(data: &[u8]) -> Option<Self> {
        if data.len() < 200 {
            return None;
        }

        let data = &data[8..];

        let liquidity_offset = 48;
        
        if data.len() < liquidity_offset + 200 {
            return None;
        }

        let mint_bytes = &data[liquidity_offset..liquidity_offset + 32];
        let mint_pubkey = Pubkey::new_from_array(
            mint_bytes.try_into().ok()?
        );

        let decimals = data.get(liquidity_offset + 32)?;

        let oracle_offset = liquidity_offset + 32 + 1 + 32 + 32;
        if data.len() < oracle_offset + 32 {
            return None;
        }
        
        let oracle_bytes = &data[oracle_offset..oracle_offset + 32];
        let oracle_pubkey = Pubkey::new_from_array(
            oracle_bytes.try_into().ok()?
        );

        let price_offset = oracle_offset + 32 + 8 + 16 + 16;
        let market_price = if data.len() >= price_offset + 16 {
            u128::from_le_bytes(
                data[price_offset..price_offset + 16].try_into().unwrap_or([0; 16])
            )
        } else {
            0
        };

        let token_name = Self::extract_token_name_from_data(data).unwrap_or_else(|| {
            Self::generate_name_from_mint(&mint_pubkey.to_string())
        });

        Some(ReserveData {
            mint_pubkey,
            decimals: *decimals,
            market_price,
            oracle_pubkey,
            token_name,
        })
    }

    fn extract_token_name_from_data(data: &[u8]) -> Option<String> {
        for chunk in data.chunks(32) {
            if chunk.len() == 32 {
                if let Some(null_pos) = chunk.iter().position(|&b| b == 0) {
                    if null_pos > 0 && null_pos < 20 {
                        if let Ok(name) = std::str::from_utf8(&chunk[..null_pos]) {
                            if name.chars().all(|c| c.is_alphanumeric() || ".-_".contains(c)) && name.len() >= 2 {
                                return Some(name.to_string());
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn generate_name_from_mint(mint: &str) -> String {
        match mint {
            "So11111111111111111111111111111111111111112" => "SOL".to_string(),
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" => "USDC".to_string(),
            "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" => "USDT".to_string(),
            "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So" => "mSOL".to_string(),
            "7dHbWXmci3dT8UFYWYZweBLXgycu7Y3iL6trKn1Y7ARj" => "stSOL".to_string(),
            "bSo13r4TkiE4KumL71LsHTPpL2euBYLFx6h9HP3piy1" => "bSOL".to_string(),
            "J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn" => "jitoSOL".to_string(),
            "7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs" => "ETH".to_string(),
            "9n4nbM75f5Ui33ZbPYXn59EwSgE8CGsHtAeTH5YFeJ9E" => "BTC".to_string(),
            _ => format!("TOKEN_{}", &mint[..8])
        }
    }
}