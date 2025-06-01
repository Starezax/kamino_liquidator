use anyhow::Result;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use solana_account_decoder::UiAccountEncoding;
use borsh::BorshDeserialize;
use solana_client::rpc_config::RpcAccountInfoConfig;
use crate::kamino::Obligation;
use log::{info, error, warn, debug};

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

    let accounts = rpc_client.get_program_accounts_with_config(program_id, config)?;
    info!("Found {} accounts matching filters", accounts.len());

    let mut obligations = Vec::new();
    for (pubkey, account) in accounts {
        if account.owner != *program_id || account.data.len() <= 8 {
            continue;
        }

        match Obligation::try_from_slice(&account.data[8..]) {
            Ok(obligation) => {
                if obligation.lending_market == *lending_market {
                    obligations.push((obligation, pubkey));
                }
            }
            Err(_) => continue,
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

pub async fn get_token_symbols_from_reserves(
    rpc_client: &RpcClient,
    program_id: &Pubkey,
    reserve_addresses: Vec<Pubkey>,
) -> Vec<String> {
    if reserve_addresses.is_empty() {
        return Vec::new();
    }

    let accounts = match rpc_client.get_multiple_accounts(&reserve_addresses) {
        Ok(accounts) => accounts,
        Err(_) => return vec!["ERROR".to_string(); reserve_addresses.len()],
    };
    
    let mut symbols = Vec::new();
    
    for (i, account_opt) in accounts.iter().enumerate() {
        let symbol = match account_opt {
            Some(account) => {
                if account.owner == *program_id && account.data.len() > 160 {
                    // Try to extract mint from offset 128 (TypeScript method)
                    if let Some(mint) = try_extract_mint_from_reserve(&account.data) {
                        generate_symbol_from_mint(&mint.to_string())
                    } else {
                        "PARSE_FAIL".to_string()
                    }
                } else {
                    "INVALID".to_string()
                }
            }
            None => "NOT_FOUND".to_string(),
        };
        
        symbols.push(symbol);
    }
    
    symbols
}

// Try to extract mint from known offset
fn try_extract_mint_from_reserve(data: &[u8]) -> Option<Pubkey> {
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

pub fn generate_symbol_from_mint(mint_address: &str) -> String {
    // Simple mapping for known tokens, otherwise generate from address
    match mint_address {
        "So11111111111111111111111111111111111111112" => "SOL".to_string(),
        "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" => "USDC".to_string(),
        "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" => "USDT".to_string(),
        "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So" => "mSOL".to_string(),
        "7dHbWXmci3dT8UFYWYZweBLXgycu7Y3iL6trKn1Y7ARj" => "stSOL".to_string(),
        "bSo13r4TkiE4KumL71LsHTPpL2euBYLFx6h9HP3piy1" => "bSOL".to_string(),
        "J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn" => "jitoSOL".to_string(),
        "7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs" => "ETH".to_string(),
        "9n4nbM75f5Ui33ZbPYXn59EwSgE8CGsHtAeTH5YFeJ9E" => "BTC".to_string(),
        "A9mUU4qviSctJVPJdBJWkb28deg915LYJKrzQ19ji3FM" => "USDCet".to_string(),
        "Gh9ZwEmdLJ8DscKNTkTqPbNwLNNBjuSzaG9Vp2KGtKJr" => "USDCpo".to_string(),
        _ => {
            // Generate a symbol from the last 4 characters of the mint address
            let len = mint_address.len();
            if len >= 4 {
                format!("TOK{}", &mint_address[len-4..].to_uppercase())
            } else {
                "UNK".to_string()
            }
        }
    }
}