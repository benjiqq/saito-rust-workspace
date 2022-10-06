use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::RwLock;
use tracing::info;

use saito_core::common::command::NetworkEvent;
use saito_core::core::data::blockchain::Blockchain;
use saito_core::core::data::mempool::Mempool;
use saito_core::core::data::msg::message::Message;
use saito_core::core::data::transaction::Transaction;
use saito_core::core::data::wallet::Wallet;

use crate::saito::config_handler::SpammerConfigs;
use crate::saito::transaction_generator::{GeneratorState, TransactionGenerator};
use crate::IoEvent;

pub struct Spammer {
    blockchain: Arc<RwLock<Blockchain>>,
    mempool: Arc<RwLock<Mempool>>,
    wallet: Arc<RwLock<Wallet>>,
    sender_to_network: Sender<IoEvent>,
    configs: Arc<RwLock<Box<SpammerConfigs>>>,
    bootstrap_done: bool,
    sent_tx_count: u64,
    tx_generator: TransactionGenerator,
}

impl Spammer {
    pub async fn new(
        blockchain: Arc<RwLock<Blockchain>>,
        mempool: Arc<RwLock<Mempool>>,
        wallet: Arc<RwLock<Wallet>>,
        sender_to_network: Sender<IoEvent>,
        sender: Sender<Vec<Transaction>>,
        configs: Arc<RwLock<Box<SpammerConfigs>>>,
    ) -> Spammer {
        Spammer {
            blockchain,
            mempool,
            wallet: wallet.clone(),
            sender_to_network,
            configs: configs.clone(),
            bootstrap_done: false,
            sent_tx_count: 0,
            tx_generator: TransactionGenerator::create(wallet.clone(), configs.clone(), sender)
                .await,
        }
    }

    async fn run(&mut self, mut receiver: Receiver<Vec<Transaction>>) {
        let mut work_done = false;
        let timer_in_milli;
        let burst_count;

        {
            let config = self.configs.read().await;
            timer_in_milli = config.get_spammer_configs().timer_in_milli;
            burst_count = config.get_spammer_configs().burst_count;
        }
        let sender = self.sender_to_network.clone();
        tokio::spawn(async move {
            loop {
                // for _i in 0..burst_count {
                if let Some(transactions) = receiver.recv().await {
                    for tx in transactions {
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
                    }
                }
                // }
                tokio::time::sleep(Duration::from_millis(timer_in_milli)).await;
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
    blockchain: Arc<RwLock<Blockchain>>,
    mempool: Arc<RwLock<Mempool>>,
    wallet: Arc<RwLock<Wallet>>,
    sender_to_network: Sender<IoEvent>,
    configs: Arc<RwLock<Box<SpammerConfigs>>>,
) {
    info!("starting the spammer");
    let (sender, receiver) = tokio::sync::mpsc::channel::<Vec<Transaction>>(1_000_000);
    let mut spammer = Spammer::new(
        blockchain,
        mempool,
        wallet,
        sender_to_network,
        sender,
        configs,
    )
    .await;
    spammer.run(receiver).await;
}
