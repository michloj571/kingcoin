use std::cmp::min;
use std::fmt::Debug;
use std::mem;

use chrono::{DateTime, Utc};
use serde::{ser::SerializeStruct, Serialize, Serializer};
use sha2::{Digest, Sha512};

use crate::blockchain::{self, BlockchainData, Transaction};
use crate::BlockHash;
use crate::network::communication::{BlockchainDto, BlockDto};

type CommitTime = Option<DateTime<Utc>>;
pub type BlockPointer<T> = Option<Box<Block<T>>>;


pub trait Summary {
    fn summary(&self) -> String;
}

pub trait BlockchainError : Debug{
    fn message(&self) -> String;
}

pub trait Validate<T> where T: BlockchainData {
    fn block_valid(&self, block: &BlockCandidate<T>) -> Result<(), Box<dyn BlockchainError>>;
}

#[derive(Debug)]
pub struct TransactionCountError {
    required_count: u64,
    actual_count: u64,
}

#[derive(Debug)]
pub struct BlockValidationError {
    block_summary: String,
    message: String,
}

#[derive(Debug)]
pub struct BlockCreationError;

pub struct BlockAdditionResult {
    block_number: u64,
    block_hash: BlockHash,
}


#[derive(Copy, Clone, PartialEq)]
pub struct BlockKey {
    hash: BlockHash,
    previous_hash: Option<BlockHash>,
}

#[derive(Serialize)]
pub struct BlockCandidate<T> where T: BlockchainData {
    key: BlockKey,
    block_number: u64,
    data: Vec<T>,
    time: DateTime<Utc>,
}

pub struct Block<T> where T: BlockchainData {
    previous_block: BlockPointer<T>,
    data: Vec<T>,
    key: BlockKey,
    time: CommitTime,
    block_number: u64,
}

pub struct Blockchain<T> where T: BlockchainData {
    last_block: BlockPointer<T>,
    chain_length: u64,
    uncommitted_data: Vec<T>,
    data_units_per_block: u64,
    remaining_pool: i64,
}

impl BlockchainError for BlockValidationError {
    fn message(&self) -> String {
        format!(
            "Block: {},\n
             error: {}",
            self.block_summary, self.message
        )
    }
}

impl BlockValidationError {
    pub(crate) fn new(block_summary: String, error: &str) -> BlockValidationError {
        BlockValidationError {
            block_summary,
            message: error.to_string(),
        }
    }
}

impl TransactionCountError {
    pub fn new(required_count: u64, actual_count: u64) -> TransactionCountError {
        TransactionCountError {
            required_count,
            actual_count,
        }
    }
}

impl BlockchainError for BlockCreationError {
    fn message(&self) -> String {
        "Only genesis block can have no ancestor".to_string()
    }
}

impl BlockchainError for TransactionCountError {
    fn message(&self) -> String {
        format!(
            "Required transactions per block: {}, actual: {}",
            self.required_count, self.actual_count
        )
    }
}

impl ToString for Transaction {
    fn to_string(&self) -> String {
        self.summary()
    }
}


impl Summary for Transaction {
    fn summary(&self) -> String {
        serde_json::to_string(&self).unwrap()
    }
}

impl Default for BlockKey {
    fn default() -> Self {
        BlockKey {
            hash: [0; 64],
            previous_hash: None,
        }
    }
}

impl ToString for BlockKey {
    fn to_string(&self) -> String {
        let previous_hash = match &self.previous_hash {
            None => {
                String::from("BEGIN")
            }
            Some(value) => {
                array_bytes::bytes2hex("", value)
            }
        };
        format!(
            "{}:{}",
            array_bytes::bytes2hex("", previous_hash),
            array_bytes::bytes2hex("", self.hash)
        )
    }
}

impl Serialize for BlockKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        let mut state = serializer.serialize_struct("BlockKey", 2)?;
        let hash = array_bytes::bytes2hex("", self.hash);
        let previous_hash = match &self.previous_hash {
            None => None,
            Some(hash) => {
                Some(array_bytes::bytes2hex("", hash))
            }
        };
        state.serialize_field("hash", &hash)?;
        state.serialize_field("previous_hash", &previous_hash)?;
        state.end()
    }
}

impl BlockKey {
    fn parse_from_dto<T>(block_dto: &mut BlockDto<T>) -> BlockKey where T: BlockchainData {
        BlockKey {
            hash: array_bytes::hex2array(block_dto.take_block_hash()).unwrap(),
            previous_hash: match block_dto.take_previous_block_hash() {
                None => None,
                Some(previous_hash) => Some(array_bytes::hex2array(previous_hash).unwrap())
            },
        }
    }

    fn hash_to_string(value: BlockHash) -> String {
        array_bytes::bytes2hex("", value)
    }

    pub fn raw_hash(&self) -> BlockHash {
        self.hash
    }

    pub fn hash(&self) -> String {
        BlockKey::hash_to_string(self.hash)
    }

    pub fn previous_hash(&self) -> Option<String> {
        match self.previous_hash {
            None => None,
            Some(hash) => Some(array_bytes::bytes2hex("", hash))
        }
    }
}

impl<T> BlockCandidate<T> where T: BlockchainData {
    pub fn take_data(&mut self) -> Vec<T> {
        mem::take(&mut self.data)
    }

    pub fn take_time(&mut self) -> DateTime<Utc> {
        mem::take(&mut self.time)
    }

    pub fn block_number(&mut self) -> u64 {
        self.block_number
    }

    pub fn key(&self) -> BlockKey {
        self.key
    }
    pub fn data(&self) -> &Vec<T> {
        &self.data
    }
    pub fn time(&self) -> DateTime<Utc> {
        self.time
    }

    pub fn create_new(
        data: Vec<T>, previous_block: &BlockPointer<T>,
    ) -> Result<BlockCandidate<T>, Box<dyn BlockchainError>> {
        match previous_block {
            None => Err(
                Box::new(
                    BlockCreationError
                )),
            Some(previous_block) => {
                let key = BlockCandidate::<T>::hash(
                    previous_block.key, BlockCandidate::summarize(&data),
                );
                Ok(BlockCandidate {
                    key,
                    block_number: previous_block.block_number + 1,
                    data,
                    time: Utc::now(),
                })
            }
        }
    }

    pub fn summarize(data: &[T]) -> String where T: BlockchainData {
        data.iter()
            .map(|data| data.summary())
            .collect::<String>()
    }

    pub fn hash(previous_key: BlockKey, data_summary: String) -> BlockKey {
        match previous_key.previous_hash {
            None => BlockKey::default(),
            Some(matched) => {
                let mut hasher = Sha512::new();
                hasher.update(matched);
                hasher.update(data_summary.as_bytes());
                let hash: BlockHash = hasher.finalize()
                    .try_into()
                    .expect("Wrong output length");
                BlockKey {
                    hash,
                    previous_hash: Some(matched),
                }
            }
        }
    }
}

impl<T> Block<T> where T: BlockchainData + Summary {
    fn new(
        previous_block: BlockPointer<T>,
        data: Vec<T>,
        block_number: u64,
        key: BlockKey,
    ) -> Block<T> {
        Block {
            previous_block,
            data,
            key,
            time: Some(Utc::now()),
            block_number,
        }
    }

    pub fn key(&self) -> BlockKey {
        self.key
    }

    pub fn previous_block(&self) -> &BlockPointer<T> {
        &self.previous_block
    }

    pub fn data(&self) -> &Vec<T> {
        &self.data
    }

    pub fn time(&self) -> CommitTime {
        self.time
    }

    pub fn block_number(&self) -> u64 {
        self.block_number
    }
}

impl<T> From<BlockDto<T>> for BlockCandidate<T> where T: BlockchainData {
    fn from(mut dto: BlockDto<T>) -> Self {
        Self {
            data: dto.take_data(),
            key: BlockKey::parse_from_dto(&mut dto),
            time: dto.take_time(),
            block_number: dto.block_number(),
        }
    }
}

impl<T> From<BlockCandidate<T>> for Block<T> where T: BlockchainData {
    fn from(mut block_candidate: BlockCandidate<T>) -> Self {
        Self {
            previous_block: None,
            data: block_candidate.take_data(),
            key: block_candidate.key(),
            time: Some(block_candidate.take_time()),
            block_number: block_candidate.block_number(),
        }
    }
}

impl<T> From<BlockchainDto<T>> for Blockchain<T> where T: BlockchainData {
    fn from(mut dto: BlockchainDto<T>) -> Self {
        let last_block = {
            let mut last_block = None;
            let block_dtos = dto.take_blocks();
            for mut block_dto in block_dtos {
                let block = Block {
                    previous_block: last_block,
                    data: block_dto.take_data(),
                    key: BlockKey::parse_from_dto(&mut block_dto),
                    time: Some(block_dto.take_time()),
                    block_number: block_dto.block_number(),
                };
                last_block = Some(Box::new(block));
            }
            last_block
        };
        Self {
            last_block,
            chain_length: dto.chain_length(),
            uncommitted_data: dto.take_uncommitted_data(),
            data_units_per_block: dto.max_data_units_per_block(),
            remaining_pool: dto.remaining_pool(),
        }
    }
}

impl<T> Summary for BlockCandidate<T> where T: BlockchainData {
    fn summary(&self) -> String {
        let mut transactions = String::new();
        self.data.iter()
            .map(|data| data.summary())
            .map(|summary| summary + ",\n")
            .for_each(|summary| transactions.push_str(&summary));
        format!(
            "Timestamp: {},
             Transaction summary: {},
             {}",
            self.time, transactions,
            self.key().to_string()
        )
    }
}

impl<T> Blockchain<T> where T: BlockchainData {
    fn new(genesis_block: Block<T>, data_units_per_block: u64, remaining_pool: i64) -> Blockchain<T> {
        Blockchain {
            last_block: Some(Box::new(genesis_block)),
            chain_length: 0,
            uncommitted_data: vec![],
            data_units_per_block,
            remaining_pool,
        }
    }

    pub fn transaction_chain(genesis_transactions: Vec<Transaction>) -> Blockchain<Transaction> {
        let to_mint: i64 = genesis_transactions.iter()
            .filter(|transaction| transaction.source_address == blockchain::MINTING_WALLET_ADDRESS)
            .map(|transaction| transaction.amount)
            .sum();

        let genesis_block = Block::new(
            None, genesis_transactions, 0, BlockKey::default(),
        );

        let mut blockchain = Blockchain::new(
            genesis_block, 2 * blockchain::TRANSACTIONS_PER_BLOCK,
            21000000
        );
        blockchain.mint(to_mint);
        blockchain
    }

    pub fn last_block(&self) -> &BlockPointer<T> {
        &self.last_block
    }

    pub fn chain_length(&self) -> u64 {
        self.chain_length
    }

    pub fn data_units_per_block(&self) -> u64 {
        self.data_units_per_block
    }

    pub fn uncommitted_data(&self) -> &[T] {
        &self.uncommitted_data[..]
    }

    fn remove_uncommitted_data(&mut self) {
        let limit = min(
            self.uncommitted_data.len(),
            self.data_units_per_block as usize
        );
        self.uncommitted_data.drain(..limit).count();
    }

    fn append_block(&mut self, mut block: Block<T>) -> BlockAdditionResult {
        let block_number = self.chain_length;
        let block_hash = block.key.hash;
        block.block_number = block_number;
        match &mut self.last_block {
            None => {
                self.last_block = Some(Box::new(block));
            }
            Some(tail) => {
                let old_tail = mem::replace(tail, Box::new(block));
                tail.previous_block = Some(old_tail);
            }
        }
        self.chain_length += 1;
        BlockAdditionResult {
            block_number,
            block_hash,
        }
    }

    pub fn add_uncommitted(&mut self, data: T) {
        self.uncommitted_data.push(data);
    }

    pub fn mint(&mut self, amount: i64) -> i64 {
        if amount <= self.remaining_pool {
            self.remaining_pool -= amount;
            self.remaining_pool
        } else {
            0
        }
    }

    pub fn remaining_pool(&self) -> i64 {
        self.remaining_pool
    }

    pub fn has_enough_uncommitted_data(&self) -> bool {
        self.uncommitted_data.len() == self.data_units_per_block as usize
    }

    pub fn submit_new_block(
        &mut self, block_candidate: BlockCandidate<T>,
    ) -> BlockAdditionResult {
        let block = Block::from(block_candidate);
        self.remove_uncommitted_data();
        self.append_block(block)
    }
}