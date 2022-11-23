use chrono::{DateTime, Utc};
use sha2::{Sha512, Digest};

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
    amount: i64,
    // in Kingcoin's smallest unit
    time: DateTime<Utc>,
}

pub struct Block {
    previous_block: BlockPointer,
    data: Vec<Transaction>,
    hash: BlockHash,
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

//todo consider introducing designated types
type CommitTime = Option<DateTime<Utc>>;
type BlockHash = Option<[u8; 64]>;
type BlockPointer = Option<Box<Block>>;

impl Block {
    fn new(
        previous_block: BlockPointer,
        data: Vec<Transaction>,
        block_number: i64,
    ) -> Block {
        Block {
            previous_block,
            data,
            hash: None,
            time: None,
            nonce: 0,
            block_number,
        }
    }

    fn genesis_block(data: Vec<Transaction>) -> Block {
        Block::new(None, data, 0)
    }

    fn summarize_transactions(transactions: &Vec<Transaction>) -> String {
        transactions.iter()
            .map(|transaction| transaction.get_summary())
            .map(|summary| summary + ",\n")
            .collect::<String>()
    }

    fn hash(transaction_summary: String, nonce: i64) -> [u8; 64] {
        let mut hasher = Sha512::new();
        hasher.update(transaction_summary.as_bytes());
        hasher.update(&nonce.to_be_bytes());
        hasher.finalize()
            .as_slice()
            .try_into()
            .expect("Wrong output length")
    }

    pub fn hash_block(&mut self, nonce: i64) {
        let transaction_summary = Block::summarize_transactions(&self.data);
        self.hash = Some(Block::hash(transaction_summary, nonce));
        self.nonce = nonce;
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
             Transaction summary: {}",
            self.time.unwrap(), transactions
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

    pub fn submit_block(&mut self, mut block: Block) -> Result<BlockAdditionResult, Box<dyn BlockchainError>> {
        match self.validator.block_valid(&block) {
            Ok(_) => {
                let block_hash = block.hash.unwrap();
                block.time = Some(Utc::now());
                self.chain_length += 1;
                match &self.last_block {
                    None => Ok(BlockAdditionResult {
                        block_number: 0,
                        block_hash,
                    }),
                    Some(block) => Ok(BlockAdditionResult {
                        block_number: self.chain_length - 1,
                        block_hash,
                    })
                }
            }
            Err(error) => Err(error)
        }
    }
}