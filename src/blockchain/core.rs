use std::mem;
use chrono::{DateTime, Utc};
use sha2::{Sha512, Digest};

//todo consider introducing designated types
type CommitTime = Option<DateTime<Utc>>;
type BlockPointer = Option<Box<Block>>;

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
    fn block_valid(&self, block: &Block) -> Result<(), Box<dyn BlockchainError>>;
}

pub struct BlockValidationError {
    block_summary: String,
    message: String,
}

pub struct BlockAdditionResult {
    block_number: i64,
    block_hash: [u8; 64],
}
pub struct Transaction {
    source_address: String,
    target_address: String,
    message: String,
    // in Kingcoin's smallest unit
    amount: i64,
    time: DateTime<Utc>,
}

#[derive(Copy, Clone)]
pub struct BlockKey {
    //todo consider moving timestamp here
    hash: [u8; 64],
    previous_hash: Option<[u8; 64]>,
}

pub struct Block {
    previous_block: BlockPointer,
    data: Vec<Transaction>,
    key: BlockKey,
    time: CommitTime,
    nonce: i64,
    block_number: i64,
}

pub struct Blockchain {
    last_block: BlockPointer,
    chain_length: i64,
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
    fn new(block: &Block, error: String) -> BlockValidationError {
        BlockValidationError {
            block_summary: block.get_summary(),
            message: error,
        }
    }
}

impl Transaction {
    pub fn new(
        source_address: String,
        target_address: String,
        message: String,
        amount: i64,
        time: DateTime<Utc>,
    ) -> Transaction {
        Transaction {
            source_address,
            target_address,
            message,
            amount,
            time,
        }
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
            self.source_address,
            self.target_address,
            self.message, self.amount,
            self.time
        )
    }
}

impl ToString for Transaction {
    fn to_string(&self) -> String {
        self.get_summary()
    }
}

impl ToString for BlockKey {
    fn to_string(&self) -> String {
        todo!() // hash to string representation
    }
}

impl BlockKey {
    fn empty() -> BlockKey {
        BlockKey {
            hash: [0; 64],
            previous_hash: None,
        }
    }

    fn attach_previous(&mut self, block: &Block) {
        self.previous_hash = block.get_key().previous_hash;
    }

    fn hash_to_string(value: [u8; 64]) -> String {
        todo!()
    }

    pub fn get_raw_hash(&self) -> [u8; 64] {
        self.hash
    }

    pub fn get_hash(&self) -> String {
        BlockKey::hash_to_string(self.hash)
    }
}

impl Block {
    fn new(
        previous_block: BlockPointer,
        data: Vec<Transaction>,
        block_number: i64,
        key: BlockKey,
    ) -> Block {
        Block {
            previous_block,
            data,
            key,
            time: None,
            nonce: 0,
            block_number,
        }
    }

    fn submit_transactions(data: Vec<Transaction>) -> Block {
        Block::new(
            None, data, -1, BlockKey::empty(),
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
        let value: [u8; 64] = hasher.finalize()
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

impl Summary<Block> for Block {
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

impl Blockchain {
    fn new(validator: Box<dyn Validate>, criteria: Box<dyn Criteria>) -> Blockchain {
        Blockchain {
            last_block: None,
            chain_length: 0,
            validator,
            criteria,
        }
    }

    fn append_block(&mut self, mut block: Block) -> BlockAdditionResult {
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

    pub fn submit(&mut self, mut block: Block) -> Result<BlockAdditionResult, Box<dyn BlockchainError>> {
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