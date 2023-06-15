// saito analytics

use ahash::AHashMap;
use saito_core::core::data::block::{Block, BlockType};
use saito_core::core::data::blockchain::{bit_pack, bit_unpack, Blockchain};
use saito_core::core::data::crypto::generate_keys;
use saito_core::core::data::wallet::Wallet;
use saito_core::{lock_for_read, lock_for_write};
use serde_json;
use std::fs;
use std::io::prelude::*;
use std::io::{self, Read};
use std::io::{Error, ErrorKind};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

use log::{debug, error, info, trace, warn};

use saito_core::common::defs::{SaitoPrivateKey, SaitoPublicKey, SaitoSignature, SaitoUTXOSetKey};

use saito_core::common::defs::{
    push_lock, Currency, SaitoHash, Timestamp, UtxoSet, GENESIS_PERIOD, LOCK_ORDER_MEMPOOL,
    LOCK_ORDER_WALLET, MAX_STAKER_RECURSION, MIN_GOLDEN_TICKETS_DENOMINATOR,
    MIN_GOLDEN_TICKETS_NUMERATOR, PRUNE_AFTER_BLOCKS,
};
use saito_core::common::defs::{LOCK_ORDER_BLOCKCHAIN, LOCK_ORDER_CONFIGS};
use std::fs::File;
use std::io::Write;

mod analyse;
mod chain_manager;
mod sutils;
mod test_io_handler;

use crate::sutils::*;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

//take blocks from a directory and output a utxo dump file
async fn runDump() {
    //read from provided directory
    let directory_path = "../../sampleblocks";
    let tresh = 1000000;

    let mut t = chain_manager::ChainManager::new();
    t.show_info();

    let blocks_result = get_blocks(directory_path);

    match blocks_result.as_ref() {
        Ok(blocks) => {
            println!("Got {} blocks", blocks.len());
            for block in blocks {
                t.add_block(block.clone()).await;
            }
        }
        Err(e) => {
            eprintln!("Error reading blocks: {}", e);
        }
    };

    t.dump_utxoset(tresh).await;
}

fn pretty_print_blocks() {
    let directory_path = "../../sampleblocks";

    let blocks_result = get_blocks(directory_path);

    match blocks_result.as_ref() {
        Ok(blocks) => {
            println!("Got {} blocks", blocks.len());
            for block in blocks {
                if let Err(e) = pretty_print_block(&block) {
                    eprintln!("Error pretty printing block: {}", e);
                }
            }
        }
        Err(e) => {
            //eprintln!("Error reading blocks: {}", e);
            eprintln!("Error ");
        }
    };
}

fn sum_balances(b: &AHashMap<String, u64>) -> u64 {
    let mut sumv = 0;
    for (key, value) in b {
        sumv += value;
    }
    sumv
}


#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("run analytics");

    //TODO make sure blockchain cases run with the ChainManager

    //TODO simple test case
    // create tx
    // create block
    // apply tx
    // check utxobalances are correct

    let mut t = chain_manager::ChainManager::new();
    let vip_amount = 1_000_000;
    let vec_tx = t.generate_tx(1, vip_amount).await;
    pretty_print_tx(&vec_tx[0]);
    pretty_print_slip(&vec_tx[0].to[0]);

    //create map from vec
    let tx_map = t.txmap_fromvec(&vec_tx).await;

    let parent_hash = [0; 32];
    //let gt = None;

    let new_block = t.make_block_simple_xy(&vec_tx).await;
    println!("new block {:?}", new_block);

    //t.initialize(viptx, vipamount).await;

    //create vec tx
    //create one block
    //apply

    //test apply works

    //runDump().await;

    //pretty_print_blocks();

    //static analysis
    //analyse::runAnalytics();

    //////////
    //simulated blocks
    // let viptx = 10;
    // let mut t = chain_manager::ChainManager::new();
    // t.initialize(viptx, 1_000_000_000).await;
    // t.wait_for_mining_event().await;

    // {
    //     let (blockchain, _blockchain_) = lock_for_read!(t.blockchain_lock, LOCK_ORDER_BLOCKCHAIN);
    // }
    // t.check_blockchain().await;

    // println!("... get_blocks ...");

    // let v = t.get_blocks().await;
    // println!("{}", v.len());

    // let b = t.get_utxobalances(0).await;
    // println!("{}", b.len());

    // let mut sumv = sum_balances(&b);
    // println!("{}", sumv);
    //assert_eq(1_000_000_000, sumv);

    // t.dump_utxoset(20000000000).await;
    // t.dump_utxoset(10000000000).await;
    // t.dump_utxoset(0).await;

    Ok(())
}

#[tokio::test]
async fn test_chain_manager() {
    // let viptx = 10;
    // let mut t = chain_manager::ChainManager::new();
    // t.initialize(viptx, 1_000_000).await;
    // t.wait_for_mining_event().await;
    // t.check_blockchain().await;

    // let v = t.get_blocks().await;
    // println!("{}", v.len());

    // let b = t.get_utxobalances(0).await;
    // println!("{}", b.len());

    // let sumv = sum_balances(&b);
    // println!("{}", sumv);
    // assert_eq!(1_000_000, sumv);
}

#[tokio::test]
async fn test_chain_manager_basic_tx() {
    // let mut t = chain_manager::ChainManager::new();
    // let vip_amount = 1_000_000_000;
    // let vec_tx = t.generate_tx(1, vip_amount).await;
    // assert_eq!(vec_tx.len(), 1);
    //pretty_print_tx(&vec_tx[0]);
}

#[tokio::test]
async fn initialize_blockchain_test() {
    let mut t = chain_manager::ChainManager::new();

    // create first block, with 100 VIP txs with 1_000_000_000 NOLAN each
    t.initialize(100, 1_000_000_000).await;
    t.wait_for_mining_event().await;

    {
        let (blockchain, _blockchain_) = lock_for_read!(t.blockchain_lock, LOCK_ORDER_BLOCKCHAIN);
        assert_eq!(1, blockchain.get_latest_block_id());
    }
    //t.check_blockchain().await;
    //t.check_utxoset().await;
    //t.check_token_supply().await;
}

#[tokio::test]
#[serial_test::serial]
//
// test we can produce five blocks in a row
//
async fn add_five_good_blocks() {
    // let filter = tracing_subscriber::EnvFilter::from_default_env();
    // let fmt_layer = tracing_subscriber::fmt::Layer::default().with_filter(filter);
    //
    // tracing_subscriber::registry().with(fmt_layer).init();

    let mut t = chain_manager::ChainManager::new();
    let block1;
    let block1_id;
    let block1_hash;
    let ts;

    //
    // block 1
    //
    t.initialize(100, 1_000_000_000).await;

    {
        let (blockchain, _blockchain_) = lock_for_write!(t.blockchain_lock, LOCK_ORDER_BLOCKCHAIN);
        block1 = blockchain.get_latest_block().unwrap();
        block1_id = block1.id;
        block1_hash = block1.hash;
        ts = block1.timestamp;

        assert_eq!(blockchain.get_latest_block_hash(), block1_hash);
        assert_eq!(blockchain.get_latest_block_id(), block1_id);
        assert_eq!(blockchain.get_latest_block_id(), 1);
    }

    //
    // block 2
    //
    let mut block2 = t
        .make_block(
            block1_hash, // hash of parent block
            ts + 120000, // timestamp
            0,           // num transactions
            0,           // amount
            0,           // fee
            true,        // mine golden ticket
        )
        .await;
    block2.generate(); // generate hashes

    let block2_hash = block2.hash;
    let block2_id = block2.id;

    t.add_block(block2).await;

    {
        let (blockchain, _blockchain_) = lock_for_write!(t.blockchain_lock, LOCK_ORDER_BLOCKCHAIN);

        assert_ne!(blockchain.get_latest_block_hash(), block1_hash);
        assert_ne!(blockchain.get_latest_block_id(), block1_id);
        assert_eq!(blockchain.get_latest_block_hash(), block2_hash);
        assert_eq!(blockchain.get_latest_block_id(), block2_id);
        assert_eq!(blockchain.get_latest_block_id(), 2);
    }

    //
    // block 3
    //
    let mut block3 = t
        .make_block(
            block2_hash, // hash of parent block
            ts + 240000, // timestamp
            0,           // num transactions
            0,           // amount
            0,           // fee
            true,        // mine golden ticket
        )
        .await;
    block3.generate(); // generate hashes

    let block3_hash = block3.hash;
    let block3_id = block3.id;

    t.add_block(block3).await;

    {
        let (blockchain, _blockchain_) = lock_for_write!(t.blockchain_lock, LOCK_ORDER_BLOCKCHAIN);

        assert_ne!(blockchain.get_latest_block_hash(), block1_hash);
        assert_ne!(blockchain.get_latest_block_id(), block1_id);
        assert_ne!(blockchain.get_latest_block_hash(), block2_hash);
        assert_ne!(blockchain.get_latest_block_id(), block2_id);
        assert_eq!(blockchain.get_latest_block_hash(), block3_hash);
        assert_eq!(blockchain.get_latest_block_id(), block3_id);
        assert_eq!(blockchain.get_latest_block_id(), 3);
    }

    //
    // block 4
    //
    let mut block4 = t
        .make_block(
            block3_hash, // hash of parent block
            ts + 360000, // timestamp
            0,           // num transactions
            0,           // amount
            0,           // fee
            true,        // mine golden ticket
        )
        .await;
    block4.generate(); // generate hashes

    let block4_hash = block4.hash;
    let block4_id = block4.id;

    t.add_block(block4).await;

    {
        let (blockchain, _blockchain_) = lock_for_write!(t.blockchain_lock, LOCK_ORDER_BLOCKCHAIN);

        assert_ne!(blockchain.get_latest_block_hash(), block1_hash);
        assert_ne!(blockchain.get_latest_block_id(), block1_id);
        assert_ne!(blockchain.get_latest_block_hash(), block2_hash);
        assert_ne!(blockchain.get_latest_block_id(), block2_id);
        assert_ne!(blockchain.get_latest_block_hash(), block3_hash);
        assert_ne!(blockchain.get_latest_block_id(), block3_id);
        assert_eq!(blockchain.get_latest_block_hash(), block4_hash);
        assert_eq!(blockchain.get_latest_block_id(), block4_id);
        assert_eq!(blockchain.get_latest_block_id(), 4);
    }

    //
    // block 5
    //
    let mut block5 = t
        .make_block(
            block4_hash, // hash of parent block
            ts + 480000, // timestamp
            0,           // num transactions
            0,           // amount
            0,           // fee
            true,        // mine golden ticket
        )
        .await;
    block5.generate(); // generate hashes

    let block5_hash = block5.hash;
    let block5_id = block5.id;

    t.add_block(block5).await;

    {
        let (blockchain, _blockchain_) = lock_for_write!(t.blockchain_lock, LOCK_ORDER_BLOCKCHAIN);

        assert_ne!(blockchain.get_latest_block_hash(), block1_hash);
        assert_ne!(blockchain.get_latest_block_id(), block1_id);
        assert_ne!(blockchain.get_latest_block_hash(), block2_hash);
        assert_ne!(blockchain.get_latest_block_id(), block2_id);
        assert_ne!(blockchain.get_latest_block_hash(), block3_hash);
        assert_ne!(blockchain.get_latest_block_id(), block3_id);
        assert_ne!(blockchain.get_latest_block_hash(), block4_hash);
        assert_ne!(blockchain.get_latest_block_id(), block4_id);
        assert_eq!(blockchain.get_latest_block_hash(), block5_hash);
        assert_eq!(blockchain.get_latest_block_id(), block5_id);
        assert_eq!(blockchain.get_latest_block_id(), 5);
    }

    t.check_blockchain().await;
    //t.check_utxoset().await;
    //t.check_token_supply().await;

    {
        let (wallet, _wallet_) = lock_for_read!(t.wallet_lock, LOCK_ORDER_WALLET);
        let count = wallet.get_unspent_slip_count();
        assert_ne!(count, 0);
        let balance = wallet.get_available_balance();
        assert_ne!(balance, 0);
    }
}

