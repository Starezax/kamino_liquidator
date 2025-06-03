mod utils;
mod kamino;

use anyhow::Result;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use std::str::FromStr;
use env_logger;
use std::time::Duration;
use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;

#[derive(Serialize, Deserialize, Debug)]
struct ObligationInfo {
    obligation_address: String,
    owner: String,
    deposited_value: String,
    borrowed_value: String,
    allowed_borrow_value: String,
    unhealthy_borrow_value: String,
    elevation_group: u8,
    has_debt: bool,
    active_deposits_count: usize,
    active_borrows_count: usize,
    deposits: Vec<DepositInfo>,
    borrows: Vec<BorrowInfo>,
    all_token_mints: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct DepositInfo {
    reserve_address: String,
    token_mint: String,
    deposited_amount: u64,
    market_value: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct BorrowInfo {
    reserve_address: String,
    token_mint: String,
    borrowed_amount: String,
    market_value: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .init();
    
    dotenv::from_path(std::path::PathBuf::from(".env")).ok();
    
    println!("Starting Lending Liquidator");
    
    let rpc_url = std::env::var("RPC_URL").expect("RPC_URL must be set");
    
    let rpc_client = RpcClient::new_with_timeout_and_commitment(
        rpc_url,
        Duration::from_secs(60),
        CommitmentConfig::confirmed()
    );
    
    let program_id = Pubkey::from_str("KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD")?;
    let lending_market = Pubkey::from_str("7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF")?;
    
    println!("Fetching obligations for lending market...");
    let mut obligations = utils::get_all_obligations_for_market(&rpc_client, &program_id, &lending_market).await?;
    
    if obligations.is_empty() {
        println!("No obligations found using filters, trying fallback method...");
        obligations = utils::get_all_program_accounts(&rpc_client, &program_id, &lending_market).await?;
    }
    
    println!("Found {} obligations total", obligations.len());
    
    let obligations_with_borrows = utils::filter_obligations_with_borrows(obligations);
    
    println!("Found {} obligations with active borrows", obligations_with_borrows.len());
    
    if obligations_with_borrows.is_empty() {
        println!("No obligations with borrows found. Exiting.");
        return Ok(());
    }
    
    println!("Collecting unique reserve addresses...");
    let mut all_reserve_addresses = HashSet::new();
    for (obligation, _) in &obligations_with_borrows {
        let reserve_addresses = obligation.get_reserve_addresses();
        for addr in reserve_addresses {
            all_reserve_addresses.insert(addr);
        }
    }
    
    let unique_reserves: Vec<Pubkey> = all_reserve_addresses.into_iter().collect();
    println!("Found {} unique reserves", unique_reserves.len());
    
    println!("Fetching reserves in batches...");
    let reserve_to_mint_map = utils::create_reserve_to_mint_mapping(&rpc_client, &program_id, unique_reserves).await?;
    
    let mut obligations_info = Vec::new();
    
    println!("Processing {} obligations...", obligations_with_borrows.len());
    
    for (obligation, address) in &obligations_with_borrows {
        let reserve_addresses = obligation.get_reserve_addresses();
        let all_token_mints: Vec<String> = reserve_addresses
            .iter()
            .map(|addr| reserve_to_mint_map.get(addr).cloned().unwrap_or_else(|| "UNKNOWN".to_string()))
            .collect();
        
        let deposits: Vec<DepositInfo> = obligation.deposits
            .iter()
            .filter(|deposit| deposit.deposit_reserve != Pubkey::default())
            .map(|deposit| {
                let token_mint = reserve_to_mint_map.get(&deposit.deposit_reserve)
                    .cloned()
                    .unwrap_or_else(|| "UNKNOWN".to_string());
                
                DepositInfo {
                    reserve_address: deposit.deposit_reserve.to_string(),
                    token_mint,
                    deposited_amount: deposit.deposited_amount,
                    market_value: deposit.market_value_sf.to_string(),
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
                
                BorrowInfo {
                    reserve_address: borrow.borrow_reserve.to_string(),
                    token_mint,
                    borrowed_amount: borrow.borrowed_amount_sf.to_string(),
                    market_value: borrow.market_value_sf.to_string(),
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
        };
        
        obligations_info.push(obligation_info);
    }

    let json_string = serde_json::to_string_pretty(&obligations_info)?;
    let mut file = File::create("obligations_detailed.json")?;
    file.write_all(json_string.as_bytes())?;
    
    println!("Successfully wrote {} obligations to obligations_detailed.json", obligations_info.len());

    Ok(())
}