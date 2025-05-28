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

#[derive(Serialize, Deserialize, Debug)]
struct ObligationJson {
    address: String,
    owner: String,
    deposited_value: String,
    borrowed_value: String,
    allowed_borrow_value: String,
    unhealthy_borrow_value: String,
    active_deposits_count: usize,
    active_borrows_count: usize,
    borrows: Vec<BorrowJson>,
}

#[derive(Serialize, Deserialize, Debug)]
struct BorrowJson {
    borrow_number: usize,
    reserve: String,
    borrowed_amount: String,
    market_value: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .init();
    
    // Load .env variables if present
    dotenv::from_path(std::path::PathBuf::from(".env")).ok();
    
    info!("Starting Lending Liquidator");
    
    let rpc_url = std::env::var("RPC_URL").expect("RPC_URL must be set");
    let rpc_client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
    
    let program_id = Pubkey::from_str("KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD")?;
    let lending_market = Pubkey::from_str("7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF")?;
    
    info!("Fetching obligations for lending market: {}", lending_market);
    let mut obligations = utils::get_all_obligations_for_market(&rpc_client, &program_id, &lending_market).await?;
    
    if obligations.is_empty() {
        warn!("No obligations found using filters, trying fallback method");
        obligations = utils::get_all_program_accounts(&rpc_client, &program_id, &lending_market).await?;
    }
    
    info!("Found {} obligations total", obligations.len());
    
    // Filter for obligations with active borrows
    let obligations_with_borrows = utils::filter_obligations_with_borrows(obligations);
    
    info!("Found {} obligations with active borrows", obligations_with_borrows.len());
    
    let mut obligations_json = Vec::new();
    
    // Process obligations and convert to JSON format
    for (i, (obligation, address)) in obligations_with_borrows.iter().enumerate() {
        info!("Processing obligation with borrows #{} - {}", i+1, address);
        
        // Count active deposits and borrows
        let active_deposits_count = obligation.deposits.iter()
            .filter(|deposit| deposit.deposit_reserve != Pubkey::default())
            .count();
        
        let active_borrows = obligation.borrows.iter()
            .filter(|borrow| borrow.borrow_reserve != Pubkey::default() && borrow.borrowed_amount_sf > 0)
            .collect::<Vec<_>>();
        
        // Convert borrows to JSON format
        let mut borrows_json = Vec::new();
        for (j, borrow) in active_borrows.iter().enumerate() {
            let borrow_json = BorrowJson {
                borrow_number: j + 1,
                reserve: borrow.borrow_reserve.to_string(),
                borrowed_amount: borrow.borrowed_amount_sf.to_string(),
                market_value: borrow.market_value_sf.to_string(),
            };
            borrows_json.push(borrow_json);
        }
        
        // Create obligation JSON
        let obligation_json = ObligationJson {
            address: address.to_string(),
            owner: obligation.owner.to_string(),
            deposited_value: obligation.deposited_value_sf.to_string(),
            borrowed_value: obligation.borrowed_assets_market_value_sf.to_string(),
            allowed_borrow_value: obligation.allowed_borrow_value_sf.to_string(),
            unhealthy_borrow_value: obligation.unhealthy_borrow_value_sf.to_string(),
            active_deposits_count,
            active_borrows_count: active_borrows.len(),
            borrows: borrows_json,
        };
        
        obligations_json.push(obligation_json);
        
        info!("  Owner: {}", obligation.owner);
        info!("  Deposited value: {}", obligation.deposited_value_sf);
        info!("  Borrowed value: {}", obligation.borrowed_assets_market_value_sf);
        info!("  Allowed borrow value: {}", obligation.allowed_borrow_value_sf);
        info!("  Unhealthy borrow value: {}", obligation.unhealthy_borrow_value_sf);
        info!("  Active deposits: {}", active_deposits_count);
        info!("  Active borrows: {}", active_borrows.len());
        
        // Display information about each active borrow
        for (j, borrow) in active_borrows.iter().enumerate() {
            info!("    Borrow #{}: Reserve: {}, Amount: {}, Market Value: {}", 
                j+1, 
                borrow.borrow_reserve, 
                borrow.borrowed_amount_sf, 
                borrow.market_value_sf
            );
        }
    }
    
    // Write to JSON file
    let json_string = serde_json::to_string_pretty(&obligations_json)?;
    let mut file = File::create("obligations_with_borrows.json")?;
    file.write_all(json_string.as_bytes())?;
    
    info!("Successfully wrote {} obligations to obligations_with_borrows.json", obligations_json.len());

    Ok(())
}