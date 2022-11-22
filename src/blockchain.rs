use std::rc::Rc;
use sha2::digest::generic_array;
use sha2::{Sha512, Digest};

pub struct Transaction {
    source_address: String,
    target_address: String,
    message: String,
    amount: i64, // in Kingcoin's smallest unit
}

impl Transaction {
    pub fn new(
        source_address: String,
        target_address: String,
        message: String,
        amount: i64,
    ) -> Transaction {
        Transaction {
            source_address,
            target_address,
            message,
            amount,
        }
    }

    fn get_summary(&self) -> String {
        format!(
            "(*{}++{}++{}++{}*)",
            self.source_address,
            self.target_address,
            self.message, self.amount
        )
    }
}

impl ToString for Transaction {
    fn to_string(&self) -> String {
        self.get_summary()
    }
}

pub struct Block {
    previous_block: Option<Rc<Block>>,
    data: Vec<Transaction>,
    hash: [u8; 64],
    next_block: Option<Box<Block>>,
}

impl Block {
    fn new(
        previous_block: Option<Rc<Block>>,
        data: Vec<Transaction>,
    ) -> Block {
        let hash = Block::hash_block(&data);
        Block {
            previous_block,
            data, hash,
            next_block: None,
        }
    }

    fn hash_block(data: &Vec<Transaction>) -> [u8; 64] {
        todo!()
    }
}

pub struct Blockchain {
    genesis: Block,
    last_block: Block,
    chain_length: i64
}