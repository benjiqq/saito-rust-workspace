
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::{Receiver, Sender};
use std::time::{SystemTime, UNIX_EPOCH};
use ahash::AHashMap;

use saito_core::core::data::wallet::Wallet;
use saito_core::core::data::blockchain::Blockchain;
use saito_core::core::data::configuration::{Configuration, PeerConfig, Server};
use saito_core::core::data::crypto::{generate_keys, generate_random_bytes, hash, verify_signature};
use saito_core::core::data::golden_ticket::GoldenTicket;
use saito_core::core::data::mempool::Mempool;
use saito_core::core::data::network::Network;
use saito_core::core::data::peer_collection::PeerCollection;
use saito_core::core::data::storage::Storage;
use saito_core::core::data::transaction::{Transaction, TransactionType};
use saito_core::core::mining_thread::MiningEvent;
use saito_core::common::defs::{
    push_lock, Currency, SaitoHash, SaitoPrivateKey, SaitoPublicKey, SaitoSignature, Timestamp,
    UtxoSet, LOCK_ORDER_BLOCKCHAIN, LOCK_ORDER_CONFIGS, LOCK_ORDER_MEMPOOL, LOCK_ORDER_WALLET,
};
use saito_core::core::data::block::Block;
use saito_core::{lock_for_read, lock_for_write};


use std::fmt::{Debug, Formatter};

//use crate::stub_iohandler::test_function;
//use crate::TestIOHandler;
use crate::stub_iohandler::TestIOHandler;

pub fn create_timestamp() -> Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as Timestamp
}

pub struct ChainManager {
    pub mempool_lock: Arc<RwLock<Mempool>>,
    pub blockchain_lock: Arc<RwLock<Blockchain>>,
    pub wallet_lock: Arc<RwLock<Wallet>>,
    pub latest_block_hash: SaitoHash,
    pub network: Network,
    pub peers: Arc<RwLock<PeerCollection>>,
    pub storage: Storage,
    pub sender_to_miner: Sender<MiningEvent>,
    pub receiver_in_miner: Receiver<MiningEvent>,
    pub configs: Arc<RwLock<dyn Configuration + Send + Sync>>,
}

struct TestConfiguration {}

impl Debug for TestConfiguration {
    fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

impl Configuration for TestConfiguration {
    fn get_server_configs(&self) -> Option<&Server> {
        todo!()
    }

    fn get_peer_configs(&self) -> &Vec<PeerConfig> {
        todo!()
    }

    fn get_block_fetch_url(&self) -> String {
        todo!()
    }

    fn is_spv_mode(&self) -> bool {
        false
    }

    fn is_browser(&self) -> bool {
        false
    }

    fn replace(&mut self, _config: &dyn Configuration) {
        todo!()
    }
}

impl ChainManager {
    pub fn new() -> Self {
        println!("create new");
        let keys = generate_keys();
        let wallet = Wallet::new(keys.1, keys.0);
        let _public_key = wallet.public_key.clone();
        let _private_key = wallet.private_key.clone();
        
        let wallet_lock = Arc::new(RwLock::new(wallet));
        let blockchain_lock = Arc::new(RwLock::new(Blockchain::new(Arc::clone(&wallet_lock))));
        let mempool_lock = Arc::new(RwLock::new(Mempool::new(Arc::clone(&wallet_lock))));
        let t = TestIOHandler::new();
        let configs = Arc::new(RwLock::new(TestConfiguration {}));
        let peers = Arc::new(RwLock::new(PeerCollection::new()));
        let (sender_to_miner, receiver_in_miner) = tokio::sync::mpsc::channel(10);
        
        Self {            
            wallet_lock: Arc::clone(&wallet_lock),
            blockchain_lock: blockchain_lock,
            mempool_lock: mempool_lock,
            latest_block_hash: [0; 32],
            network: Network::new(
                Box::new(TestIOHandler::new()),
                Arc::clone(&peers),
                wallet_lock.clone(),
                configs.clone(),
            ),
            peers: Arc::clone(&peers),
            storage: Storage::new(Box::new(TestIOHandler::new())),
            sender_to_miner: sender_to_miner.clone(),
            receiver_in_miner: receiver_in_miner,
            configs,
        }
    }

    pub async fn add_block(&mut self, block: Block) {
        debug!("adding block to test manager blockchain");
        let (configs, _configs_) = lock_for_read!(self.configs, LOCK_ORDER_CONFIGS);
        let (mut blockchain, _blockchain_) =
            lock_for_write!(self.blockchain_lock, LOCK_ORDER_BLOCKCHAIN);
        let (mut mempool, _mempool_) = lock_for_write!(self.mempool_lock, LOCK_ORDER_MEMPOOL);

        blockchain
            .add_block(
                block,
                &mut self.network,
                &mut self.storage,
                self.sender_to_miner.clone(),
                &mut mempool,
                configs.deref(),
            )
            .await;
        debug!("block added to test manager blockchain");
    }

    pub async fn create_block(
        &mut self,
        parent_hash: SaitoHash,
        timestamp: Timestamp,
        txs_number: usize,
        txs_amount: Currency,
        txs_fee: Currency,
        include_valid_golden_ticket: bool,
    ) -> Block {
        let mut transactions: AHashMap<SaitoSignature, Transaction> = Default::default();
        let private_key: SaitoPrivateKey;
        let public_key: SaitoPublicKey;

        {
            let (wallet, _wallet_) = lock_for_read!(self.wallet_lock, LOCK_ORDER_WALLET);

            public_key = wallet.public_key;
            private_key = wallet.private_key;
        }

        for _i in 0..txs_number {
            let mut transaction;
            {
                let (mut wallet, _wallet_) =
                    lock_for_write!(self.wallet_lock, LOCK_ORDER_WALLET);

                transaction =
                    Transaction::create(&mut wallet, public_key, txs_amount, txs_fee, false)
                        .unwrap();
            }

            transaction.sign(&private_key);
            transaction.generate(&public_key, 0, 0);
            transactions.insert(transaction.signature, transaction);
        }

        if include_valid_golden_ticket {
            let (blockchain, _blockchain_) =
                lock_for_read!(self.blockchain_lock, LOCK_ORDER_BLOCKCHAIN);

            let block = blockchain.get_block(&parent_hash).unwrap();
            let golden_ticket: GoldenTicket = Self::create_golden_ticket(
                self.wallet_lock.clone(),
                parent_hash,
                block.difficulty,
            )
            .await;
            let mut gttx: Transaction;
            {
                let (wallet, _wallet_) = lock_for_read!(self.wallet_lock, LOCK_ORDER_WALLET);

                gttx = Wallet::create_golden_ticket_transaction(
                    golden_ticket,
                    &wallet.public_key,
                    &wallet.private_key,
                )
                .await;
            }
            gttx.generate(&public_key, 0, 0);
            transactions.insert(gttx.signature, gttx);
        }

        let (configs, _configs_) = lock_for_read!(self.configs, LOCK_ORDER_CONFIGS);
        let (mut blockchain, _blockchain_) =
            lock_for_write!(self.blockchain_lock, LOCK_ORDER_BLOCKCHAIN);
        //
        // create block
        //
        let mut block = Block::create(
            &mut transactions,
            parent_hash,
            blockchain.borrow_mut(),
            timestamp,
            &public_key,
            &private_key,
            None,
            configs.deref(),
        )
        .await;
        block.generate();
        block.sign(&private_key);

        block
    }

    pub async fn create_golden_ticket(
        wallet: Arc<RwLock<Wallet>>,
        block_hash: SaitoHash,
        block_difficulty: u64,
    ) -> GoldenTicket {
        let public_key;
        {
            let (wallet, _wallet_) = lock_for_read!(wallet, LOCK_ORDER_WALLET);

            public_key = wallet.public_key;
        }
        let mut random_bytes = hash(&generate_random_bytes(32));

        let mut gt = GoldenTicket::create(block_hash, random_bytes, public_key);

        while !gt.validate(block_difficulty) {
            random_bytes = hash(&generate_random_bytes(32));
            gt = GoldenTicket::create(block_hash, random_bytes, public_key);
        }

        GoldenTicket::new(block_hash, random_bytes, public_key)
    }

    pub async fn initialize(&mut self, vip_transactions: u64, vip_amount: Currency) {
        let timestamp = create_timestamp();
        self.initialize_with_timestamp(vip_transactions, vip_amount, timestamp)
            .await;
    }

    pub async fn initialize_with_timestamp(
        &mut self,
        vip_transactions: u64,
        vip_amount: Currency,
        timestamp: Timestamp,
    ) {
        //
        // initialize timestamp
        //

        //
        // reset data dirs
        //
        tokio::fs::remove_dir_all("data/blocks").await;
        tokio::fs::create_dir_all("data/blocks").await.unwrap();
        tokio::fs::remove_dir_all("data/wallets").await;
        tokio::fs::create_dir_all("data/wallets").await.unwrap();

        //
        // create initial transactions
        //
        let private_key: SaitoPrivateKey;
        let public_key: SaitoPublicKey;
        {
            let (wallet, _wallet_) = lock_for_read!(self.wallet_lock, LOCK_ORDER_WALLET);

            public_key = wallet.public_key;
            private_key = wallet.private_key;
        }

        //
        // create first block
        //
        let mut block = self.create_block([0; 32], timestamp, 0, 0, 0, false).await;

        //
        // generate UTXO-carrying VIP transactions
        //
        for _i in 0..vip_transactions {
            let mut tx = Transaction::create_vip_transaction(public_key, vip_amount);
            tx.generate(&public_key, 0, 0);
            tx.sign(&private_key);
            block.add_transaction(tx);
        }

        {
            let (configs, _configs_) = lock_for_read!(self.configs, LOCK_ORDER_CONFIGS);
            // we have added VIP, so need to regenerate the merkle-root
            block.merkle_root =
                block.generate_merkle_root(configs.is_browser(), configs.is_spv_mode());
        }
        block.generate();
        block.sign(&private_key);

        assert!(verify_signature(
            &block.pre_hash,
            &block.signature,
            &block.creator,
        ));

        // and add first block to blockchain
        self.add_block(block).await;
    }
}

