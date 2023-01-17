use std::collections::{HashMap, HashSet};
use std::mem;

use chrono::{DateTime, Utc};
use libp2p::{PeerId, Swarm};
use serde::{Deserialize, Serialize};

use crate::blockchain::{BlockchainData, StakeBid, Transaction, Wallet};
use crate::blockchain::core::{BlockCandidate, Blockchain, Summary};
use crate::network::{BlockchainBehaviour, NETWORK_TOPIC};

pub mod dispatch;

#[derive(Eq, PartialEq, Hash)]
pub struct Vote {
    id: PeerId,
    block_valid: bool,
}

impl Vote {
    pub fn new(id: PeerId, block_valid: bool) -> Vote {
        Vote {
            id,
            block_valid,
        }
    }

    pub fn block_valid(&self) -> bool {
        self.block_valid
    }
}

pub struct VotingResult {
    block_valid: i64,
    block_invalid: i64,
}

impl VotingResult {
    pub fn evaluate(block_valid: i64, block_invalid: i64) -> VotingResult {
        VotingResult {
            block_valid,
            block_invalid,
        }
    }

    pub fn should_append_block(&self) -> bool {
        self.block_valid > self.block_invalid
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockDto<T> where T: BlockchainData {
    block_hash: String,
    previous_block_hash: Option<String>,
    data: Vec<T>,
    time: DateTime<Utc>,
    block_number: u64,
}

#[derive(Serialize, Deserialize)]
pub struct BlockchainDto<T> where T: BlockchainData {
    blocks: Vec<BlockDto<T>>,
    chain_length: u64,
    uncommitted_data: Vec<T>,
    max_data_units_per_block: u64,
    remaining_pool: i64,
}

impl<T> BlockchainDto<T> where T: BlockchainData {
    pub fn take_blocks(&mut self) -> Vec<BlockDto<T>> {
        mem::take(&mut self.blocks)
    }
    pub fn chain_length(&self) -> u64 {
        self.chain_length
    }
    pub fn take_uncommitted_data(&mut self) -> Vec<T> {
        mem::take(&mut self.uncommitted_data)
    }
    pub fn max_data_units_per_block(&self) -> u64 {
        self.max_data_units_per_block
    }
    pub fn remaining_pool(&self) -> i64 {
        self.remaining_pool
    }
}

impl<T> From<&mut Blockchain<T>> for BlockchainDto<T> where T: BlockchainData {
    fn from(blockchain: &mut Blockchain<T>) -> Self {
        let blocks = {
            let mut current_block = blockchain.last_block();
            let mut result: Vec<BlockDto<T>> = vec![];
            loop {
                match current_block {
                    None => break,
                    Some(block) => {
                        let block_key = block.key();
                        let block_dto = BlockDto {
                            block_hash: block_key.hash(),
                            previous_block_hash: block_key.previous_hash(),
                            data: block.data().clone(),
                            time: block.time().clone().unwrap(),
                            block_number: block.block_number(),
                        };
                        result.push(block_dto);
                        current_block = block.previous_block();
                    }
                }
            };
            result
        };
        Self {
            blocks,
            chain_length: blockchain.chain_length(),
            uncommitted_data: blockchain.uncommitted_data().to_vec(),
            max_data_units_per_block: blockchain.data_units_per_block(),
            remaining_pool: blockchain.remaining_pool(),
        }
    }
}

impl<T> BlockDto<T> where T: BlockchainData {
    pub fn take_block_hash(&mut self) -> String {
        mem::take(&mut self.block_hash)
    }

    pub fn take_previous_block_hash(&mut self) -> Option<String> {
        mem::take(&mut self.previous_block_hash)
    }

    pub fn take_data(&mut self) -> Vec<T> {
        mem::take(&mut self.data)
    }

    pub fn take_time(&mut self) -> DateTime<Utc> {
        mem::take(&mut self.time)
    }

    pub fn block_number(&self) -> u64 {
        self.block_number
    }
}

impl<T> From<BlockCandidate<T>> for BlockDto<T> where T: BlockchainData + Summary {
    fn from(mut candidate: BlockCandidate<T>) -> Self {
        let block_key = candidate.key();
        Self {
            block_hash: block_key.hash(),
            previous_block_hash: block_key.previous_hash(),
            data: candidate.take_data(),
            time: candidate.take_time(),
            block_number: candidate.block_number(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub enum BlockchainMessage {
    Join(Wallet),
    JoinDenied,
    Sync {
        transactions: BlockchainDto<Transaction>,
        wallets: HashSet<Wallet>,
        stakes: BlockchainDto<Transaction>,
    },
    SubmitTransaction {
        transaction: Transaction,
        transaction_fee: Transaction
    },
    SubmitBlock {
        block_dto: BlockDto<Transaction>
    },
    Vote {
        block_valid: bool
    },
    Bid(StakeBid),
}


pub fn publish_message(swarm: &mut Swarm<BlockchainBehaviour>, message: BlockchainMessage) {
    let message = serde_json::to_string(&message).unwrap();
    let sending_result = swarm.behaviour_mut()
        .gossipsub()
        .publish(NETWORK_TOPIC.clone(), message);
    match sending_result {
        Ok(_) => {}
        Err(_) => println!("Could not publish")
    }
}