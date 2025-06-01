mod utils;
mod kamino;

use anyhow::Result;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use std::str::FromStr;
use log::{info, warn};
use env_logger;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::time::Duration;
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
struct ObligationTokenInfo {
    obligation_address: String,
    owner: String,
    deposited_value: String,
    borrowed_value: String,
    allowed_borrow_value: String,
    unhealthy_borrow_value: String,
    active_deposits_count: usize,
    active_borrows_count: usize,
    reserve_addresses: Vec<String>,
    token_symbols: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .init();
    
    dotenv::from_path(std::path::PathBuf::from(".env")).ok();
    
    info!("Starting Lending Liquidator");
    
    let rpc_url = std::env::var("RPC_URL").expect("RPC_URL must be set");
    
    let rpc_client = RpcClient::new_with_timeout_and_commitment(
        rpc_url,
        Duration::from_secs(60),
        CommitmentConfig::confirmed()
    );
    
    let program_id = Pubkey::from_str("KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD")?;
    let lending_market = Pubkey::from_str("7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF")?;
    
    info!("Fetching obligations for lending market: {}", lending_market);

    let mut obligations = utils::get_all_obligations_for_market(&rpc_client, &program_id, &lending_market).await?;
    
    if obligations.is_empty() {
        warn!("No obligations found using filters, trying fallback method");
        obligations = utils::get_all_program_accounts(&rpc_client, &program_id, &lending_market).await?;
    }
    
    info!("Found {} obligations total", obligations.len());
    
    let obligations_with_borrows = utils::filter_obligations_with_borrows(obligations);
    
    info!("Found {} obligations with active borrows", obligations_with_borrows.len());
    
    // Create obligation:token HashMap
    let mut obligation_token_map: HashMap<String, ObligationTokenInfo> = HashMap::new();

    for (obligation, address) in obligations_with_borrows.iter() {
        let active_deposits_count = obligation.deposits.iter()
            .filter(|deposit| deposit.deposit_reserve != Pubkey::default())
            .count();
        
        let active_borrows_count = obligation.borrows.iter()
            .filter(|borrow| borrow.borrow_reserve != Pubkey::default() && borrow.borrowed_amount_sf > 0)
            .count();

        // Get all reserve addresses
        let reserve_addresses = obligation.get_reserve_addresses();
        let reserve_address_strings: Vec<String> = reserve_addresses.iter()
            .map(|addr| addr.to_string())
            .collect();

        // Generate token symbols from reserve addresses using mint extraction
        let token_symbols = utils::get_token_symbols_from_reserves(
            &rpc_client, 
            &program_id, 
            reserve_addresses
        ).await;

        let obligation_info = ObligationTokenInfo {
            obligation_address: address.to_string(),
            owner: obligation.owner.to_string(),
            deposited_value: obligation.deposited_value_sf.to_string(),
            borrowed_value: obligation.borrowed_assets_market_value_sf.to_string(),
            allowed_borrow_value: obligation.allowed_borrow_value_sf.to_string(),
            unhealthy_borrow_value: obligation.unhealthy_borrow_value_sf.to_string(),
            active_deposits_count,
            active_borrows_count,
            reserve_addresses: reserve_address_strings,
            token_symbols,
        };
        
        obligation_token_map.insert(address.to_string(), obligation_info);
    }

    // Save the obligation:token map
    let json_string = serde_json::to_string_pretty(&obligation_token_map)?;
    let mut file = File::create("obligation_token_map.json")?;
    file.write_all(json_string.as_bytes())?;
    
    info!("Successfully wrote {} obligation:token pairs to obligation_token_map.json", obligation_token_map.len());

    Ok(())
}