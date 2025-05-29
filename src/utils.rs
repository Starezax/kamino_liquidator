use anyhow::{Result};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use solana_account_decoder::UiAccountEncoding;
use borsh::BorshDeserialize;
use solana_client::rpc_config::RpcAccountInfoConfig;
use crate::kamino::Obligation;
use log::{info, error};

pub async fn get_all_obligations_for_market(
    rpc_client: &RpcClient,
    program_id: &Pubkey,
    lending_market: &Pubkey,
) -> Result<Vec<(Obligation, Pubkey)>> {
    info!("Fetching obligations for program: {} and lending market: {}", program_id, lending_market);
    
    let lending_market_offset = 32; // 8 (discriminator) + 8 (tag) + 16 (LastUpdate)
    
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

    info!("Fetching program accounts with lending market filter at offset {}", lending_market_offset);
    let accounts = rpc_client.get_program_accounts_with_config(program_id, config)?;
    info!("Found {} accounts matching filters", accounts.len());

    let mut obligations = Vec::new();
    for (pubkey, account) in accounts {
        info!("Processing account: {}", pubkey);
        info!("Account data length: {} bytes", account.data.len());
        
        if account.owner != *program_id {
            info!("Skipping account {} - wrong owner: {}", pubkey, account.owner);
            continue;
        }

        if account.data.len() <= 8 {
            error!("Account data too short for anchor discriminator.");
            continue;
        }


        match Obligation::try_from_slice(&account.data[8..]) {
            Ok(obligation) => {
                if obligation.lending_market == *lending_market {
                    info!("Successfully deserialized obligation for account: {}", pubkey);
                    obligations.push((obligation, pubkey));
                } else {
                    info!("Skipping account {} with unmatched lending market", pubkey);
                }
            }
            Err(e) => {
                error!("Failed to deserialize account {}: {}", pubkey, e);
            }
        }
    }

    info!("Successfully processed {} obligations", obligations.len());
    Ok(obligations)
}

pub async fn get_all_program_accounts(
    rpc_client: &RpcClient,
    program_id: &Pubkey,
    lending_market: &Pubkey,
) -> Result<Vec<(Obligation, Pubkey)>> {
    info!("Using fallback method: fetching all program accounts");
    
    let accounts = rpc_client.get_program_accounts(program_id)?;
    info!("Found {} total program accounts", accounts.len());
    
    let mut obligations = Vec::new();
    for (pubkey, account) in accounts {
        if account.data.len() <= 8 {
            continue;
        }
        if let Ok(obligation) = Obligation::try_from_slice(&account.data[8..]) {
            if obligation.lending_market == *lending_market {
                obligations.push((obligation, pubkey));
            }
        }
    }
    
    info!("Found {} valid obligations for the lending market in fallback", obligations.len());
    Ok(obligations)
}

pub fn filter_obligations_with_borrows(obligations: Vec<(Obligation, Pubkey)>) -> Vec<(Obligation, Pubkey)> {
    info!("Filtering obligations to only those with active borrows");
    
    let filtered_obligations: Vec<(Obligation, Pubkey)> = obligations
        .into_iter()
        .filter(|(obligation, _)| {
            obligation.borrows.iter().any(|borrow| {
                borrow.borrow_reserve != Pubkey::default() && borrow.borrowed_amount_sf > 0
            })
        })
        .collect();
    
    info!("Found {} obligations with active borrows", filtered_obligations.len());
    filtered_obligations
}