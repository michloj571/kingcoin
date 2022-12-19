use std::mem;
use chrono::{DateTime, Utc};
use sha2::{Sha512, Digest};
use serde::{Serialize, Deserialize};
use crate::blockchain::{Address, BlockchainData};
use crate::BlockHash;
use crate::network::Transaction;

//todo consider introducing designated types
type CommitTime = Option<DateTime<Utc>>;
type BlockPointer<T> = Option<Box<Block<T>>>;


trait Summary<T> {
    fn get_summary(&self) -> String;
}

pub trait BlockchainError {
    fn get_message(&self) -> String;
}

pub trait Criteria {
    fn criteria_fulfilled(&self, hash: &[u8]) -> bool;
}

pub trait Validate {
    fn block_valid<T>(&self, block: &Block<T>) -> Result<(), Box<dyn BlockchainError>> ;
}

pub struct BlockValidationError {
    block_summary: String,
    message: String,
}

pub struct BlockAdditionResult {
    block_number: u64,
    block_hash: BlockHash,
}


#[derive(Copy, Clone)]
pub struct BlockKey {
    //todo consider moving timestamp here
    hash: BlockHash,
    previous_hash: Option<BlockHash>,
}

pub struct Block<T> where T: BlockchainData {
    previous_block: BlockPointer<T>,
    data: Vec<T>,
    key: BlockKey,
    time: CommitTime,
    nonce: i64,
    block_number: u64,
}

pub struct Blockchain<T> where T: BlockchainData {
    last_block: BlockPointer<T>,
    chain_length: u64,
    validator: Box<dyn Validate>,
    criteria: Box<dyn Criteria>,
}

impl BlockchainError for BlockValidationError {
    fn get_message(&self) -> String {
        format!(
            "Block:{},
             error: {}",
            self.block_summary, self.message
        )
    }
}

impl BlockValidationError {
    fn new<T>(block: &Block<T>, error: String)  -> BlockValidationError where T: BlockchainData {
        BlockValidationError {
            block_summary: block.get_summary(),
            message: error,
        }
    }
}

impl ToString for Transaction {
    fn to_string(&self) -> String {
        self.get_summary()
    }
}


impl Summary<Transaction> for Transaction {
    fn get_summary(&self) -> String {
        format!(
            "[
              Source address: {},
              Target address: {},
              Message: {},
              Amount: {},
              Time: {}
             ]",
            array_bytes::bytes2hex("", self.source_address()),
            array_bytes::bytes2hex("", self.target_address()),
            self.title(), self.amount(), self.time()
        )
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

impl BlockKey {
    fn empty() -> BlockKey {
        BlockKey {
            hash: [0; 64],
            previous_hash: None,
        }
    }

    fn attach_previous<T>(&mut self, block: &Block<T>) where T: BlockchainData {
        self.previous_hash = block.get_key().previous_hash;
    }

    fn hash_to_string(value: BlockHash) -> String {
        todo!()
    }

    pub fn get_raw_hash(&self) -> BlockHash {
        self.hash
    }

    pub fn get_hash(&self) -> String {
        BlockKey::hash_to_string(self.hash)
    }
}

impl<T> Block<T> where T: BlockchainData {
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
            time: None,
            nonce: 0,
            block_number,
        }
    }

    fn submit_transactions(data: Vec<Transaction>) -> Block<T> {
        Block::new(
            None, data, 0, BlockKey::empty(),
        )
    }

    fn summarize_transactions(transactions: &Vec<Transaction>) -> String {
        transactions.iter()
            .map(|transaction| transaction.get_summary())
            .map(|summary| summary + ",\n")
            .collect::<String>()
    }

    fn hash(transaction_summary: String, nonce: i64) -> BlockKey {
        let mut hasher = Sha512::new();
        hasher.update(transaction_summary.as_bytes());
        hasher.update(&nonce.to_be_bytes());
        let value: BlockHash = hasher.finalize()
            .as_slice()
            .try_into()
            .expect("Wrong output length");
        BlockKey {
            hash: value,
            previous_hash: None,
        }
    }

    pub fn hash_block(&mut self, nonce: i64) {
        let transaction_summary = Block::summarize_transactions(&self.data);
        self.key = Block::hash(transaction_summary, nonce);
        self.nonce = nonce;
    }

    pub fn get_key(&self) -> BlockKey {
        self.key
    }
}

impl<T> Summary<Block<T>> for Block<T> where T: BlockchainData {
    fn get_summary(&self) -> String {
        let mut transactions = String::new();
        self.data.iter()
            .map(|transaction| transaction.get_summary())
            .map(|summary| summary + ",\n")
            .for_each(|summary| transactions.push_str(&summary));
        format!(
            "Timestamp: {},
             Transaction summary: {},
             {}",
            self.time.unwrap(), transactions,
            self.get_key().to_string()
        )
    }
}

impl<T> Blockchain<T> where T: BlockchainData {
    pub fn new(validator: Box<dyn Validate>, criteria: Box<dyn Criteria>) -> Blockchain<T> {
        Blockchain {
            last_block: None,
            chain_length: 0,
            validator,
            criteria,
        }
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
                tail.key.attach_previous(&old_tail);
                tail.previous_block = Some(old_tail);
            }
        }
        self.chain_length += 1;
        BlockAdditionResult {
            block_number,
            block_hash,
        }
    }

    pub fn get_criteria(&self) -> &dyn Criteria {
        &*self.criteria
    }

    pub fn update_criteria(&mut self, criteria: Box<dyn Criteria>) {
        self.criteria = criteria
    }

    //todo adapt validation to p2p
    pub fn submit(&mut self, mut block: Block<T>) -> Result<BlockAdditionResult, Box<dyn BlockchainError>> {
        match self.validator.block_valid(&block) {
            Ok(_) => {
                block.time = Some(Utc::now());
                Ok(self.append_block(block))
            }
            Err(error) => Err(error)
        }
    }

    //todo add utility function for searching in the blockchain
}