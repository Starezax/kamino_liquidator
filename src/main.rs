mod utils;
mod kamino;
mod price_listener;

use anyhow::Result;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use std::str::FromStr;
use std::time::Duration;
use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use price_listener::{PriceListener, get_current_price_info, get_token_symbol};
use tracing::info;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PriceInfo {
    pub symbol: String,
    pub price: f64,
    pub confidence: f64,
    pub status: String,
    pub last_updated: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DepositInfo {
    pub reserve_address: String,
    pub token_mint: String,
    pub token_symbol: String,
    pub deposited_amount: u64,
    pub market_value: String,
    pub live_price: Option<f64>,
    pub price_status: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BorrowInfo {
    pub reserve_address: String,
    pub token_mint: String,
    pub token_symbol: String,
    pub borrowed_amount: String,
    pub market_value: String,
    pub live_price: Option<f64>,
    pub price_status: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ObligationInfo {
    pub obligation_address: String,
    pub owner: String,
    pub deposited_value: String,
    pub borrowed_value: String,
    pub allowed_borrow_value: String,
    pub unhealthy_borrow_value: String,
    pub elevation_group: u8,
    pub has_debt: bool,
    pub active_deposits_count: usize,
    pub active_borrows_count: usize,
    pub deposits: Vec<DepositInfo>,
    pub borrows: Vec<BorrowInfo>,
    pub all_token_mints: Vec<String>,
    pub live_prices: HashMap<String, PriceInfo>,
    pub last_updated: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    
    dotenv::dotenv().ok();
    
    info!("Starting Kamino Liquidator with CONSOLIDATED Pyth Price Listener");
    info!("================================================================================");
    
    let rpc_url = std::env::var("RPC_URL").expect("RPC_URL must be set");
    
    let rpc_client = RpcClient::new_with_timeout_and_commitment(
        rpc_url,
        Duration::from_secs(60),
        CommitmentConfig::confirmed()
    );
    
    let program_id = Pubkey::from_str("KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD")?;
    let lending_market = Pubkey::from_str("7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF")?;
    
    info!("Fetching obligations for lending market...");
    let mut obligations = utils::get_all_obligations_for_market(&rpc_client, &program_id, &lending_market).await?;
    
    if obligations.is_empty() {
        info!("No obligations found using filters, trying fallback method...");
        obligations = utils::get_all_program_accounts(&rpc_client, &program_id, &lending_market).await?;
    }
    
    info!("Found {} obligations total", obligations.len());
    
    let obligations_with_borrows = utils::filter_obligations_with_borrows(obligations);
    
    info!("Found {} obligations with active borrows", obligations_with_borrows.len());
    
    if obligations_with_borrows.is_empty() {
        info!("No obligations with borrows found. Exiting.");
        return Ok(());
    }
    
    info!("Collecting unique reserve addresses...");
    let mut all_reserve_addresses = HashSet::new();
    for (obligation, _) in &obligations_with_borrows {
        let reserve_addresses = obligation.get_reserve_addresses();
        for addr in reserve_addresses {
            all_reserve_addresses.insert(addr);
        }
    }
    
    let unique_reserves: Vec<Pubkey> = all_reserve_addresses.into_iter().collect();
    info!("Found {} unique reserves", unique_reserves.len());
    
    info!("Fetching reserves in batches...");
    let reserve_to_mint_map = utils::create_reserve_to_mint_mapping(&rpc_client, &program_id, unique_reserves).await?;
    
    let all_token_mints: HashSet<String> = reserve_to_mint_map.values()
        .filter(|mint| *mint != "UNKNOWN" && *mint != "PARSE_FAIL" && *mint != "INVALID" && *mint != "NOT_FOUND")
        .cloned()
        .collect();
    
    let token_mints: Vec<String> = all_token_mints.into_iter().collect();
    info!("Found {} unique token mints", token_mints.len());
    
    let price_listener = PriceListener::new(token_mints);
    let price_listener_arc = Arc::new(price_listener);
    
    let _price_task_handle = price_listener_arc.start();
    
    info!("Starting obligation processing (waiting 30s for REAL Pyth prices)...");
    tokio::time::sleep(Duration::from_secs(30)).await;
    
    let obligations_with_borrows = Arc::new(obligations_with_borrows);
    let reserve_to_mint_map = Arc::new(reserve_to_mint_map);
    
    let mut update_counter = 0;
    
    loop {
        update_counter += 1;
        let mut obligations_info = Vec::new();
        let current_timestamp = chrono::Utc::now().to_rfc3339();
        
        info!("Processing obligations update #{}", update_counter);
        
        for (obligation, address) in obligations_with_borrows.iter() {
            let reserve_addresses = obligation.get_reserve_addresses();
            let all_token_mints: Vec<String> = reserve_addresses
                .iter()
                .map(|addr| reserve_to_mint_map.get(addr).cloned().unwrap_or_else(|| "UNKNOWN".to_string()))
                .collect();
            
            let mut live_prices: HashMap<String, PriceInfo> = HashMap::new();
            for mint in &all_token_mints {
                if let Some(price_info) = get_current_price_info(mint) {
                    live_prices.insert(mint.clone(), PriceInfo {
                        symbol: price_info.symbol,
                        price: price_info.price,
                        confidence: price_info.confidence,
                        status: price_info.status,
                        last_updated: price_info.last_updated.to_rfc3339(),
                    });
                }
            }
            
            let deposits: Vec<DepositInfo> = obligation.deposits
                .iter()
                .filter(|deposit| deposit.deposit_reserve != Pubkey::default())
                .map(|deposit| {
                    let token_mint = reserve_to_mint_map.get(&deposit.deposit_reserve)
                        .cloned()
                        .unwrap_or_else(|| "UNKNOWN".to_string());
                    
                    let token_symbol = get_token_symbol(&token_mint).to_string();
                    let (live_price, price_status) = 
                        if let Some(price_info) = get_current_price_info(&token_mint) {
                            (Some(price_info.price), Some(price_info.status))
                        } else {
                            (None, Some("No Pyth Data".to_string()))
                        };
                    
                    DepositInfo {
                        reserve_address: deposit.deposit_reserve.to_string(),
                        token_mint,
                        token_symbol,
                        deposited_amount: deposit.deposited_amount,
                        market_value: deposit.market_value_sf.to_string(),
                        live_price,
                        price_status,
                    }
                })
                .collect();
            
            let borrows: Vec<BorrowInfo> = obligation.borrows
                .iter()
                .filter(|borrow| borrow.borrow_reserve != Pubkey::default() && borrow.borrowed_amount_sf > 0)
                .map(|borrow| {
                    let token_mint = reserve_to_mint_map.get(&borrow.borrow_reserve)
                        .cloned()
                        .unwrap_or_else(|| "UNKNOWN".to_string());
                    
                    let token_symbol = get_token_symbol(&token_mint).to_string();
                    let (live_price, price_status) = 
                        if let Some(price_info) = get_current_price_info(&token_mint) {
                            (Some(price_info.price), Some(price_info.status))
                        } else {
                            (None, Some("No Pyth Data".to_string()))
                        };
                    
                    BorrowInfo {
                        reserve_address: borrow.borrow_reserve.to_string(),
                        token_mint,
                        token_symbol,
                        borrowed_amount: borrow.borrowed_amount_sf.to_string(),
                        market_value: borrow.market_value_sf.to_string(),
                        live_price,
                        price_status,
                    }
                })
                .collect();
            
            let obligation_info = ObligationInfo {
                obligation_address: address.to_string(),
                owner: obligation.owner.to_string(),
                deposited_value: obligation.deposited_value_sf.to_string(),
                borrowed_value: obligation.borrowed_assets_market_value_sf.to_string(),
                allowed_borrow_value: obligation.allowed_borrow_value_sf.to_string(),
                unhealthy_borrow_value: obligation.unhealthy_borrow_value_sf.to_string(),
                elevation_group: obligation.elevation_group,
                has_debt: obligation.has_debt != 0,
                active_deposits_count: deposits.len(),
                active_borrows_count: borrows.len(),
                deposits,
                borrows,
                all_token_mints,
                live_prices,
                last_updated: current_timestamp.clone(),
            };
            
            obligations_info.push(obligation_info);
        }

        let json_string = serde_json::to_string_pretty(&obligations_info)?;
        let mut file = File::create("obligations_with_pyth_prices.json")?;
        file.write_all(json_string.as_bytes())?;
        
        let total_live_prices: usize = obligations_info.iter()
            .map(|o| o.live_prices.len())
            .sum();
        
        info!("Updated obligations file (#{}) - {} obligations, {} Pyth prices", 
              update_counter, obligations_info.len(), total_live_prices);
        
        tokio::time::sleep(Duration::from_secs(20)).await;
    }
}