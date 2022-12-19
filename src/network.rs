use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use libp2p::{PeerId};
use libp2p::identity::Keypair;
use serde::{Serialize, Deserialize};
use crate::blockchain::{Address, BlockchainData};
use crate::blockchain::core::BlockKey;

mod communication;

lazy_static! {
pub static ref KEYS: Keypair = Keypair::generate_ed25519();
pub static ref PEER_ID: PeerId = PeerId::from(KEYS.public());
}

//todo implement p2p network

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockCandidate<T> where T: BlockchainData {
    block_key: BlockKey,
    data: Vec<T>,
    block_number: i64,
    nonce: i64,
}


#[derive(Debug, Serialize, Deserialize)]
pub struct Transaction {
    source_address: Address,
    target_address: Address,
    title: String,
    // in Kingcoin's smallest unit
    amount: u64,
    time: DateTime<Utc>,
}

impl Transaction {
    pub fn new(
        source_address: Address,
        target_address: Address,
        message: String,
        amount: u64,
        time: DateTime<Utc>,
    ) -> Transaction {
        Transaction {
            source_address,
            target_address,
            title: message,
            amount,
            time,
        }
    }
    
    pub fn source_address(&self) -> Address {
        self.source_address
    }
    pub fn target_address(&self) -> Address {
        self.target_address
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn amount(&self) -> u64 {
        self.amount
    }
    pub fn time(&self) -> DateTime<Utc> {
        self.time
    }
}

impl BlockchainData for Transaction {}