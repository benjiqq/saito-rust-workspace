use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::RwLock;
use tracing::info;

use saito_core::common::command::NetworkEvent;
use saito_core::common::defs::{push_lock, Currency, LOCK_ORDER_CONFIGS};
use saito_core::core::data::blockchain::Blockchain;
use saito_core::core::data::mempool::Mempool;
use saito_core::core::data::msg::message::Message;
use saito_core::core::data::peer_collection::PeerCollection;
use saito_core::core::data::transaction::Transaction;
use saito_core::core::data::wallet::Wallet;
use saito_core::lock_for_read;

use crate::saito::config_handler::SpammerConfigs;
use crate::saito::transaction_generator::{GeneratorState, TransactionGenerator};
use crate::IoEvent;

pub struct Spammer {
    sender_to_network: Sender<IoEvent>,
    peers: Arc<RwLock<PeerCollection>>,
    configs: Arc<RwLock<Box<SpammerConfigs>>>,
    bootstrap_done: bool,
    sent_tx_count: u64,
    tx_generator: TransactionGenerator,
}

impl Spammer {
    pub async fn new(
        wallet: Arc<RwLock<Wallet>>,
        peers: Arc<RwLock<PeerCollection>>,
        blockchain: Arc<RwLock<Blockchain>>,
        sender_to_network: Sender<IoEvent>,
        sender: Sender<VecDeque<Transaction>>,
        configs: Arc<RwLock<Box<SpammerConfigs>>>,
    ) -> Spammer {
        let tx_payment;
        let tx_fee;
        {
            let (configs, _configs_) = lock_for_read!(configs, LOCK_ORDER_CONFIGS);
            tx_payment = configs.get_spammer_configs().tx_payment;
            tx_fee = configs.get_spammer_configs().tx_fee;
        }
        Spammer {
            sender_to_network,
            peers: peers.clone(),
            configs: configs.clone(),
            bootstrap_done: false,
            sent_tx_count: 0,
            tx_generator: TransactionGenerator::create(
                wallet.clone(),
                peers.clone(),
                blockchain.clone(),
                configs.clone(),
                sender,
                tx_payment as Currency,
                tx_fee as Currency,
            )
            .await,
        }
    }

    async fn run(&mut self, mut receiver: Receiver<VecDeque<Transaction>>) {
        let mut work_done = false;
        let timer_in_milli;
        let burst_count;
        let stop_after;

        {
            let (configs, _configs_) = lock_for_read!(self.configs, LOCK_ORDER_CONFIGS);

            timer_in_milli = configs.get_spammer_configs().timer_in_milli;
            burst_count = configs.get_spammer_configs().burst_count;
            stop_after = configs.get_spammer_configs().stop_after;
        }

        let sender = self.sender_to_network.clone();
        tokio::spawn(async move {
            let mut total_count = 0;
            let mut count = burst_count;
            loop {
                if let Some(transactions) = receiver.recv().await {
                    for tx in transactions {
                        count -= 1;
                        total_count += 1;
                        sender
                            .send(IoEvent {
                                event_processor_id: 0,
                                event_id: 0,
                                event: NetworkEvent::OutgoingNetworkMessageForAll {
                                    buffer: Message::Transaction(tx).serialize(),
                                    exceptions: vec![],
                                },
                            })
                            .await
                            .unwrap();

                        if count == 0 {
                            tokio::time::sleep(Duration::from_millis(timer_in_milli)).await;
                            count = burst_count;
                        }
                        if total_count == stop_after {
                            tokio::time::sleep(Duration::from_millis(10_000)).await;
                            info!("terminating spammer after sending : {:?} txs", total_count);
                            std::process::exit(0);
                        }
                    }
                }
            }
        });
        tokio::task::yield_now().await;
        loop {
            work_done = false;
            if !self.bootstrap_done {
                self.tx_generator.on_new_block().await;
                self.bootstrap_done = (self.tx_generator.get_state() == GeneratorState::Done);
                work_done = true;
            }

            if !work_done {
                tokio::time::sleep(Duration::from_millis(timer_in_milli)).await;
            }
        }
    }
}

pub async fn run_spammer(
    wallet: Arc<RwLock<Wallet>>,
    peers: Arc<RwLock<PeerCollection>>,
    blockchain: Arc<RwLock<Blockchain>>,
    sender_to_network: Sender<IoEvent>,
    configs: Arc<RwLock<Box<SpammerConfigs>>>,
) {
    info!("starting the spammer");
    let (sender, receiver) = tokio::sync::mpsc::channel::<VecDeque<Transaction>>(10);
    let mut spammer = Spammer::new(
        wallet,
        peers,
        blockchain,
        sender_to_network,
        sender,
        configs,
    )
    .await;
    spammer.run(receiver).await;
}
