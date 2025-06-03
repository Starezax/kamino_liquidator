use anyhow::Result;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use solana_account_decoder::UiAccountEncoding;
use borsh::BorshDeserialize;
use solana_client::rpc_config::RpcAccountInfoConfig;
use crate::kamino::Obligation;
use std::collections::HashMap;

pub async fn get_all_obligations_for_market(
    rpc_client: &RpcClient,
    program_id: &Pubkey,
    lending_market: &Pubkey,
) -> Result<Vec<(Obligation, Pubkey)>> {
    let lending_market_offset = 32;
    
    let filters = vec![
        solana_client::rpc_filter::RpcFilterType::Memcmp(
            solana_client::rpc_filter::Memcmp::new_base58_encoded(
                lending_market_offset,
                lending_market.to_bytes().as_slice(),
            )
        ),
    ];

    let config = solana_client::rpc_config::RpcProgramAccountsConfig {
        filters: Some(filters),
        account_config: RpcAccountInfoConfig {
            encoding: Some(UiAccountEncoding::Base64),
            commitment: Some(CommitmentConfig::confirmed()),
            data_slice: None,
            min_context_slot: None,
        },
        with_context: Some(true),
    };

    let accounts = rpc_client.get_program_accounts_with_config(program_id, config)?;
    let mut obligations = Vec::new();
    
    for (pubkey, account) in &accounts {
        if account.owner != *program_id || account.data.len() <= 8 {
            continue;
        }

        match Obligation::try_from_slice(&account.data[8..]) {
            Ok(obligation) => {
                if obligation.lending_market == *lending_market {
                    obligations.push((obligation, *pubkey));
                }
            }
            Err(_) => continue,
        }
    }

    Ok(obligations)
}

pub async fn get_all_program_accounts(
    rpc_client: &RpcClient,
    program_id: &Pubkey,
    lending_market: &Pubkey,
) -> Result<Vec<(Obligation, Pubkey)>> {
    let accounts = rpc_client.get_program_accounts(program_id)?;
    let mut obligations = Vec::new();
    
    for (pubkey, account) in &accounts {
        if account.data.len() <= 8 {
            continue;
        }
        if let Ok(obligation) = Obligation::try_from_slice(&account.data[8..]) {
            if obligation.lending_market == *lending_market {
                obligations.push((obligation, *pubkey));
            }
        }
    }
    
    Ok(obligations)
}

pub fn filter_obligations_with_borrows(obligations: Vec<(Obligation, Pubkey)>) -> Vec<(Obligation, Pubkey)> {
    obligations
        .into_iter()
        .filter(|(obligation, _)| {
            obligation.borrows.iter().any(|borrow| {
                borrow.borrow_reserve != Pubkey::default() && borrow.borrowed_amount_sf > 0
            })
        })
        .collect()
}

pub async fn create_reserve_to_mint_mapping(
    rpc_client: &RpcClient,
    program_id: &Pubkey,
    reserve_addresses: Vec<Pubkey>,
) -> Result<HashMap<Pubkey, String>> {
    const BATCH_SIZE: usize = 100;
    let mut reserve_to_mint = HashMap::new();
    
    for chunk in reserve_addresses.chunks(BATCH_SIZE) {
        let accounts = rpc_client.get_multiple_accounts(chunk)?;
        
        for (i, account_opt) in accounts.iter().enumerate() {
            let reserve_addr = chunk[i];
            
            match account_opt {
                Some(account) => {
                    if account.owner == *program_id && account.data.len() > 160 {
                        if let Some(mint_pubkey) = try_extract_mint_from_reserve(&account.data) {
                            reserve_to_mint.insert(reserve_addr, mint_pubkey.to_string());
                        } else {
                            reserve_to_mint.insert(reserve_addr, "PARSE_FAIL".to_string());
                        }
                    } else {
                        reserve_to_mint.insert(reserve_addr, "INVALID".to_string());
                    }
                }
                None => {
                    reserve_to_mint.insert(reserve_addr, "NOT_FOUND".to_string());
                }
            }
        }
    }
    
    Ok(reserve_to_mint)
}

pub fn try_extract_mint_from_reserve(data: &[u8]) -> Option<Pubkey> {
    // From the TypeScript filter, the mint is at offset 128
    let offset = 128;
    
    if data.len() >= offset + 32 {
        let mint_bytes = &data[offset..offset + 32];
        if let Ok(mint_array) = mint_bytes.try_into() {
            let pubkey = Pubkey::new_from_array(mint_array);
            // Basic validation - check if it's not all zeros and not the system program
            if pubkey != Pubkey::default() && 
               pubkey.to_string() != "11111111111111111111111111111111" {
                return Some(pubkey);
            }
        }
    }

    // Try other common offsets as fallback
    let possible_offsets = [56, 88, 120, 160];

    for offset in possible_offsets {
        if data.len() >= offset + 32 {
            let mint_bytes = &data[offset..offset + 32];
            if let Ok(mint_array) = mint_bytes.try_into() {
                let pubkey = Pubkey::new_from_array(mint_array);
                if pubkey != Pubkey::default() && 
                   pubkey.to_string() != "11111111111111111111111111111111" {
                    return Some(pubkey);
                }
            }
        }
    }

    None
}