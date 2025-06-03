use crate::constants;
use crate::estimator::Estimator;
use crate::pool::MintPair;
use crate::pool::Pool;
use crate::pool::PoolType;
use crate::pool::PoolsByMints;
use crate::pool::PoolsByMintsExt;
use crate::pools::ticks_cache::TickArrayCache;
use borsh::BorshDeserialize;
use dashmap::DashMap;
use futures::stream::Stream;
use futures::stream::StreamExt;
use log::error;
use once_cell::sync::Lazy;
use crate::pools::orca::TickArray;
use crate::pools::orca::Whirlpool;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::RwLock;
use tracing::{info, trace, warn};
use yellowstone_grpc_proto::geyser::geyser_client::GeyserClient;
use yellowstone_grpc_proto::geyser::{
    subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest,
};
use yellowstone_grpc_proto::geyser::{
    SubscribeRequestFilterAccounts, SubscribeRequestFilterBlocksMeta, SubscribeUpdateAccount,
};

#[derive(Default, Debug)]
pub struct AppState {
    pub latest_block_hash: String,
    pub slot: u64,
}

pub static SHARED_STATE: Lazy<Arc<DashMap<(), AppState>>> =
    Lazy::new(|| Arc::new(DashMap::with_capacity(1)));

pub fn get_shared_state() -> Arc<DashMap<(), AppState>> {
    Arc::clone(&SHARED_STATE)
}

pub trait Listener {
    fn get_subscription_request(&self) -> SubscribeRequest;
    fn handle_update(&self, update: UpdateOneof);
    fn name(&self) -> String;

    // Basic implementation for launching listener with retry logic
    async fn start(self: Arc<Self>) {
        let mut pubsub_addr = constants::GRPC_URL.to_string();
        // if self.name() == "OrcaTickArrayListener" {
        //     println!("Starting OrcaTickArrayListener");
        //     pubsub_addr = constants::GRPC_URL2.to_string();
        // }
        let retry_delay = tokio::time::Duration::from_secs(2);
        let max_retries = 5;
        let mut attempt = 0;

        loop {
            attempt += 1;
            info!(
                "{} connecting to: {} (attempt {})",
                self.name(),
                pubsub_addr,
                attempt
            );

            match GeyserClient::connect(pubsub_addr.clone()).await {
                Ok(mut grpc_client) => {
                    info!("{} connected successfully to gRPC server", self.name());

                    let request = self.get_subscription_request();
                    let stream = futures::stream::once(async { request });
                    let request: Pin<Box<dyn Stream<Item = SubscribeRequest> + Send + 'static>> =
                        Box::pin(stream);
                    match grpc_client.subscribe(request).await {
                        Ok(response) => {
                            info!(
                                "{} subscribed successfully, starting to listen for updates",
                                self.name()
                            );
                            let mut response_stream = response.into_inner();

                            while let Some(update_result) = response_stream.next().await {
                                match update_result {
                                    Ok(update) => {
                                        if let Some(update_oneof) = update.update_oneof {
                                            trace!("{} received update: ", self.name());
                                            self.handle_update(update_oneof);
                                        }
                                    }
                                    Err(e) => {
                                        error!("{} stream error: {:?}", self.name(), e);
                                        break;  // Break inner loop to trigger reconnection
                                    }
                                }
                            }
                            error!("{} stream ended unexpectedly, retrying...", self.name());
                        }
                        Err(e) => {
                            error!("{} failed to subscribe: {:?}", self.name(), e);
                        }
                    }
                }
                Err(e) => {
                    error!("{} failed to connect to gRPC server: {:?}", self.name(), e);
                }
            }

            if attempt >= max_retries {
                error!(
                    "{} reached max retry attempts ({}) and will stop retrying",
                    self.name(),
                    max_retries
                );
                break;
            }

            warn!(
                "{} retrying in {} seconds...",
                self.name(),
                retry_delay.as_secs()
            );
            tokio::time::sleep(retry_delay).await;
        }
    }
}

pub struct ReserveListener {
    pub reserve_keys: Vec<Pubkey>, // specific keys to listen to
    pub dex: PoolType,             // dex type
    pub vault_to_pool_map: Arc<DashMap<Pubkey, Arc<RwLock<Pool>>>>, // used to get pool from
    pub pools_by_mints: PoolsByMints, // Main source of truth
    pub estimator: Estimator,
    pub last_updated: Arc<DashMap<Pubkey, bool>>, // Track which vault was last updated
}

impl ReserveListener {
    pub fn new(
        dex: PoolType,
        pools_by_mints: PoolsByMints,
        estimator: Estimator,
    ) -> Self {
        let active_pools = pools_by_mints.get_active_pools(dex.clone());
        let reserve_keys: Vec<Pubkey> = active_pools.clone()
            .iter()
            .filter(|pool| pool.pool_type != PoolType::Orca)
            .flat_map(|pool| vec![pool.base_vault, pool.quote_vault])
            .collect(); // Create vault to pool mapping
        
        let vault_to_pool_map = {
            let map = DashMap::new();
            let arc_pools: Vec<Arc<RwLock<Pool>>> = active_pools.clone()
                .into_iter()
                .map(|pool| Arc::new(RwLock::new(pool)))
                .collect();
            for pool_arc in &arc_pools {
                map.insert(pool_arc.read().unwrap().base_vault, Arc::clone(pool_arc));
                map.insert(pool_arc.read().unwrap().quote_vault, Arc::clone(pool_arc));
            }
            Arc::new(map)
        };
        let last_updated = Arc::new(DashMap::new());
        for pool in &active_pools {
            last_updated.insert(pool.base_vault, false);
            last_updated.insert(pool.quote_vault, false);
        }

        ReserveListener {
            reserve_keys,
            dex,
            vault_to_pool_map,
            pools_by_mints,
            estimator,
            last_updated,
        }
    }
}

impl Listener for ReserveListener {
    fn handle_update(&self, update: UpdateOneof) {
        //let start_time = std::time::Instant::now();

        if let UpdateOneof::Account(SubscribeUpdateAccount {
            account: Some(account_info),
            ..
        }) = update
        {
            let vault_pubkey = unsafe {
                Pubkey::new_from_array(account_info.pubkey.try_into().unwrap_unchecked())
            };

            // Get the pool pubkey from the vault
            let pool = self.vault_to_pool_map.get_mut(&vault_pubkey).unwrap();

            let amount = unsafe {
                let amount_ptr = account_info.data.as_ptr().add(64) as *const u64;
                *amount_ptr
            };

            // Scope the lock and get all necessary data
            let (pool_copy, price_change_pct, mint_pair) = {
                let mut pool_lock = pool
                    .write()
                    .map_err(|_| "Failed to acquire write lock".to_string())
                    .unwrap();

                // Store old amounts before updating
                let old_mint_a = pool_lock.mint_a_reserve;
                let old_mint_b = pool_lock.mint_b_reserve;

                // Update the reserves
                if pool_lock.base_vault == vault_pubkey {
                    pool_lock.mint_a_reserve = amount;
                } else if pool_lock.quote_vault == vault_pubkey {
                    pool_lock.mint_b_reserve = amount;
                }

                // Calculate price change
                let old_price = (old_mint_b as f64) / (old_mint_a as f64);
                let new_price =
                    (pool_lock.mint_b_reserve as f64) / (pool_lock.mint_a_reserve as f64);
                let price_change_pct = ((new_price - old_price) / old_price) * 100.0;

                let mint_pair = MintPair::new(pool_lock.mint_a, pool_lock.mint_b);
                let pool_copy = pool_lock.clone();

                (pool_copy, price_change_pct, mint_pair)
            }; // Lock is released here

            self.last_updated.insert(vault_pubkey, true);

            //let processing_time = start_time.elapsed().as_micros() as u64;
            // info!(
            //     target: "pool_update",
            //     "{} pool={}, mint_a_reserve={}, mint_b_reserve={}, processed in {} micro seconds",
            //     self.dex.as_str(), pool_copy.pool_id, pool_copy.mint_a_reserve,
            //     pool_copy.mint_b_reserve, processing_time
            // );

            let base_updated = *self.last_updated.get(&pool_copy.base_vault).unwrap();
            let quote_updated = *self.last_updated.get(&pool_copy.quote_vault).unwrap();

            if base_updated && quote_updated {
                // Reset the last updated flags
                self.last_updated.insert(pool_copy.base_vault, false);
                self.last_updated.insert(pool_copy.quote_vault, false);

                let all_pair_pools = self.pools_by_mints.get(&mint_pair).unwrap();

                let pools_copy = all_pair_pools.clone();
                let estimator = self.estimator.clone();
                if constants::KOLIBRI_ENABLED == false {
                    tokio::spawn(async move {
                        estimator.check_arbitrage_opportunities(
                        &pools_copy,
                        &pool_copy,
                        price_change_pct,
                        None,
                        None
                        );
                    });
                }
            }
        }
    }

    fn get_subscription_request(&self) -> SubscribeRequest {
        get_accounts_subscribe_request(self.reserve_keys.clone())
    }

    fn name(&self) -> String {
        format!("Reserve Listener for {}", self.dex.as_str())
    }
}

pub struct BlockHashListener;

impl BlockHashListener {
    pub fn new() -> Self {
        BlockHashListener
    }
}

impl Listener for BlockHashListener {
    // type FutureType = BoxFuture<'static, ()>;

    fn get_subscription_request(&self) -> SubscribeRequest {
        let blocks_meta_filter = SubscribeRequestFilterBlocksMeta::default();

        let mut blocks_meta_map = HashMap::new();
        blocks_meta_map.insert("blockmetadata".to_string(), blocks_meta_filter);

        SubscribeRequest {
            slots: HashMap::new(),
            accounts: HashMap::new(),
            transactions: HashMap::new(),
            blocks: HashMap::new(),
            blocks_meta: blocks_meta_map,
            entry: HashMap::new(),
            commitment: Some(CommitmentLevel::Confirmed as i32),
            accounts_data_slice: vec![],
        }
    }

    fn handle_update(&self, update: UpdateOneof) {
        if let UpdateOneof::BlockMeta(block_update) = update {
            let shared_state = get_shared_state();
            println!("Block hash: {}", block_update.blockhash);
            shared_state.insert(
                (),
                AppState {
                    latest_block_hash: block_update.blockhash,
                    slot: block_update.slot,
                },
            );
        }
    }

    fn name(&self) -> String {
        "BlockHashListener".to_string()
    }
}

fn get_accounts_subscribe_request(reserve_keys: Vec<Pubkey>) -> SubscribeRequest {
    // Token program ID
    let token_program_id = constants::TOKEN_PROGRAM_ID.to_string();
    let account_filter = SubscribeRequestFilterAccounts {
        account: reserve_keys.iter().map(|k| k.to_string()).collect(),
        owner: vec![token_program_id],
        filters: vec![],
        ..Default::default()
    };

    let wsol_account_string = "So11111111111111111111111111111111111111112".to_string(); // random value

    // Construct the accounts map
    let mut accounts_map: HashMap<String, SubscribeRequestFilterAccounts> = HashMap::new();
    accounts_map.insert(wsol_account_string, account_filter);

    SubscribeRequest {
        accounts: accounts_map,
        commitment: Some(CommitmentLevel::Processed as i32),
        ..Default::default() // TODO: should we listen to specific data offsets ?
    }
}
pub struct OrcaPoolStateListener {
    pub pool_ids: Vec<Pubkey>,        // specific keys to listen to
    pub pools_by_mints: PoolsByMints, // Main source of truth
    pub estimator: Estimator,
}
impl OrcaPoolStateListener {
    pub fn new(
        pools_by_mints: PoolsByMints,
        pool_ids: Vec<Pubkey>,
        estimator: Estimator,
    ) -> Self {
        OrcaPoolStateListener {
            pool_ids,
            pools_by_mints,
            estimator,
        }
    }
}
impl Listener for OrcaPoolStateListener {
    fn handle_update(&self, update: UpdateOneof) {
        if let UpdateOneof::Account(SubscribeUpdateAccount {
            account: Some(account_info),
            ..
        }) = update
        {
            let pool_id = unsafe {
                Pubkey::new_from_array(account_info.pubkey.try_into().unwrap_unchecked())
            };
            let pool = Whirlpool::try_from_slice(&account_info.data).unwrap();
            let mint_a = Pubkey::new_from_array(pool.token_mint_a.to_bytes());
            let mint_b = Pubkey::new_from_array(pool.token_mint_b.to_bytes());

            self.pools_by_mints.update_pool(
                pool,
                pool_id,
                mint_a,
                mint_b,
                &self.estimator,
            );
        }
    }

    fn get_subscription_request(&self) -> SubscribeRequest {
        let orca_program_id = constants::ORCA_WHIRLPOOL_PROGRAM.to_string();
        // probably use data size filter ? for better speed ?
        let account_filter = SubscribeRequestFilterAccounts {
            account: self.pool_ids.iter().map(|k| k.to_string()).collect(),
            owner: vec![orca_program_id],
            filters: vec![],
            ..Default::default()
        };

        let wsol_account_string = "So11111111111111111111111111111111111111112".to_string(); // random value

        // Construct the accounts map
        let mut accounts_map: HashMap<String, SubscribeRequestFilterAccounts> = HashMap::new();
        accounts_map.insert(wsol_account_string, account_filter);

        SubscribeRequest {
            accounts: accounts_map,
            commitment: Some(CommitmentLevel::Processed as i32),
            ..Default::default() // TODO: should we listen to specific data offsets ?
        }
    }
    fn name(&self) -> String {
        "OrcaPoolStateListener".to_string()
    }
}
pub struct OrcaTickArrayListener {
    pub pool_id: String,
    pub tick_array_pubkeys: Vec<Pubkey>,
    pub tick_cache: TickArrayCache,
}

impl OrcaTickArrayListener {
    pub fn new(
        pool_id: String,
        tick_array_pubkeys: Vec<Pubkey>,
        tick_cache: TickArrayCache,
    ) -> Self {
        OrcaTickArrayListener {
            pool_id,
            tick_array_pubkeys,
            tick_cache,
        }
    }
}

impl Listener for OrcaTickArrayListener {
    fn handle_update(&self, update: UpdateOneof) {
        if let UpdateOneof::Account(SubscribeUpdateAccount {
            account: Some(account_info),
            ..
        }) = update
        {
            let tick_array_pubkey = unsafe {
                Pubkey::new_from_array(account_info.pubkey.try_into().unwrap_unchecked())
            };
            
            if let Ok(tick_array) = TickArray::try_from_slice(&account_info.data) {
                // info!(
                //     "Updating tick array {} for pool {}, start_tick_index: {}", 
                //     tick_array_pubkey, 
                //     self.pool_id,
                //     tick_array.start_tick_index
                // );
                self.tick_cache.tick_arrays.insert(tick_array_pubkey, tick_array);
            }
        }
    }

    fn get_subscription_request(&self) -> SubscribeRequest {
        let orca_program_id = constants::ORCA_WHIRLPOOL_PROGRAM.to_string();
        
        let account_filter = SubscribeRequestFilterAccounts {
            account: self.tick_array_pubkeys.iter().map(|k| k.to_string()).collect(),
            owner: vec![orca_program_id],
            filters: vec![],
            ..Default::default()
        };

        let mut accounts_map: HashMap<String, SubscribeRequestFilterAccounts> = HashMap::new();
        accounts_map.insert("tick_arrays".to_string(), account_filter);

        SubscribeRequest {
            accounts: accounts_map,
            commitment: Some(CommitmentLevel::Processed as i32),
            ..Default::default()
        }
    }

    fn name(&self) -> String {
        format!("OrcaTickArrayListener for pool {}", self.pool_id)
    }
}