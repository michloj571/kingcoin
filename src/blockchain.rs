use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use rsa::{pss::VerifyingKey, RsaPublicKey, signature::{Signature, Verifier}};
use rsa::pss::BlindedSigningKey;
use rsa::rand_core::{CryptoRng, RngCore};
use rsa::signature::RandomizedSigner;
use serde::{Deserialize, Serialize};
use sha2::Sha512;

use crate::blockchain::core::{
    BlockCandidate, Blockchain, BlockchainError, BlockValidationError,
    Criteria, Summary, Validate,
};

pub mod core;

pub type Address = [u8; 32];

pub static TRANSACTION_FEE: i64 = 50;
pub static MINTING_WALLET_ADDRESS: Address = [0; 32];
lazy_static! {
    pub static ref STAKE_WALLET_ADDRESS: Address = {
        let mut address = [0;32];
        address[0] = 1;
        address
    };
}


pub trait BlockchainData: Summary + Clone + Serialize {}

#[derive(Debug, Serialize, Deserialize, Eq, Hash, PartialEq)]
pub struct Transaction {
    source_address: Address,
    target_address: Address,
    title: String,
    // in Kingcoin's smallest unit
    amount: i64,
    time: DateTime<Utc>,
    sender_signature: Option<String>,
}

impl Transaction {
    pub fn new(
        source_address: Address,
        target_address: Address,
        message: String,
        amount: i64,
        time: DateTime<Utc>,
    ) -> Transaction {
        Transaction {
            source_address,
            target_address,
            title: message,
            amount,
            time,
            sender_signature: None,
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
    pub fn amount(&self) -> i64 {
        self.amount
    }
    pub fn time(&self) -> DateTime<Utc> {
        self.time
    }
    pub fn sender_signature(&self) -> &Option<String> {
        &self.sender_signature
    }

    pub fn sign(&mut self, key: BlindedSigningKey<Sha512>, rng: impl CryptoRng + RngCore) {
        let signature = key.sign_with_rng(
            rng,
            self.signed_content().as_bytes(),
        );
        self.sender_signature = Some(signature.to_string());
    }

    pub fn signed_content(&self) -> String {
        format! {
            "{}{}{}{}",
            array_bytes::bytes2hex("", self.source_address),
            array_bytes::bytes2hex("", self.target_address),
            self.amount, self.title
        }
    }

    pub fn stake_bid(bid: i64, source_address: Address) -> Transaction {
        Transaction::new(
            source_address, *STAKE_WALLET_ADDRESS, "".to_string(),
            bid, Utc::now(),
        )
    }

    pub fn stake_return(bid: i64, target_address: Address) -> Transaction {
        Transaction::new(
            *STAKE_WALLET_ADDRESS, target_address, "".to_string(),
            bid, Utc::now(),
        )
    }
}

#[derive(PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StakeBid {
    stake: i64,
    transaction: Transaction,
}

impl StakeBid {
    pub fn bid(bid: i64, wallet_address: Address) -> StakeBid {
        StakeBid {
            stake: bid,
            transaction: Transaction::stake_bid(bid, wallet_address),
        }
    }

    pub fn stake(&self) -> i64 {
        self.stake
    }

    pub fn transaction(&self) -> &Transaction {
        &self.transaction
    }
}

impl Clone for Transaction {
    fn clone(&self) -> Self {
        Self {
            source_address: self.source_address.clone(),
            target_address: self.target_address.clone(),
            title: self.title.clone(),
            amount: self.amount,
            time: self.time.clone(),
            sender_signature: self.sender_signature.clone(),
        }
    }
}

impl BlockchainData for Transaction {}

pub struct TransactionValidator<'a> {
    wallets: &'a Blockchain<Wallet>,
    transactions: &'a Blockchain<Transaction>,
}

impl<'a> Validate<Transaction> for TransactionValidator<'a> {
    fn block_valid(&self, block: &BlockCandidate<Transaction>) -> Result<(), Box<dyn BlockchainError>> {
        let mut total_reward = 0;

        self.validate_hash(block)?;

        for transaction in block.data() {
            if transaction.source_address() != MINTING_WALLET_ADDRESS {
                let signature = match transaction.sender_signature() {
                    None => {
                        return Err(
                            Box::new(TransactionValidationError)
                        );
                    }
                    Some(signature) => signature
                };
                if transaction.source_address() == transaction.target_address() {
                    return Err(
                        Box::new(TransactionValidationError)
                    );
                }
                self.validate_transfer(transaction, &signature)?;
            } else {
                total_reward += transaction.amount;
            }
        }

        if total_reward == TRANSACTION_FEE {
            Ok(())
        } else {
            Err(Box::new(
                BlockValidationError::new(
                    serde_json::to_string_pretty(block).unwrap(),
                    "Invalid reward",
                )
            ))
        }
    }
}

impl<'a> TransactionValidator<'a> {
    pub fn new(wallets: &'a Blockchain<Wallet>, transactions: &'a Blockchain<Transaction>) -> TransactionValidator<'a> {
        Self {
            wallets,
            transactions,
        }
    }
    pub fn wallets(&self) -> &Blockchain<Wallet> {
        &self.wallets
    }

    fn validate_hash(
        &self, block_candidate: &BlockCandidate<Transaction>,
    ) -> Result<(), Box<dyn BlockchainError>> {
        let given_key = block_candidate.key();

        let computed = BlockCandidate::<Transaction>::hash(
            given_key, BlockCandidate::summarize(block_candidate.data()),
        );

        if computed.previous_hash() == given_key.previous_hash()
            && computed.hash() == given_key.hash() {
            Ok(())
        } else {
            Err(Box::new(
                BlockValidationError::new(
                    serde_json::to_string_pretty(block_candidate).unwrap(),
                    "Invalid hash",
                )
            ))
        }
    }

    fn validate_transfer(
        &self, transaction: &Transaction, signature: &str,
    ) -> Result<(), Box<dyn BlockchainError>> {
        let source_wallet = find_wallet_by_address(
            transaction.source_address(), &self.wallets,
        );

        match find_wallet_by_address(transaction.target_address(), &self.wallets) {
            None => return Err(
                Box::new(TransactionValidationError)
            ),
            Some(wallet) => wallet
        };

        match source_wallet {
            None => return Err(
                Box::new(TransactionValidationError)
            ),
            Some(wallet) => {
                let available_balance = wallet.balance(self.transactions);
                let public_key = wallet.key()
                    .clone()
                    .unwrap();
                let key: VerifyingKey<Sha512> = VerifyingKey::from(public_key);
                let verified = key.verify(
                    transaction.signed_content().as_bytes(),
                    &Signature::from_bytes(signature.as_bytes()).unwrap())
                    .is_err();
                if !verified || available_balance < transaction.amount {
                    return Err(
                        Box::new(TransactionValidationError)
                    );
                } else {
                    Ok(())
                }
            }
        }
    }
}

pub struct TransactionCriteria;

impl Criteria for TransactionCriteria {
    fn criteria_fulfilled(&self, hash: &[u8]) -> bool {
        true
    }
}

pub struct BlockCriteria;

impl Criteria for BlockCriteria {
    fn criteria_fulfilled(&self, hash: &[u8]) -> bool {
        let hash = array_bytes::bytes2hex("", hash);
        hash.starts_with("000000")
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Wallet {
    address: [u8; 32],
    public_key: Option<RsaPublicKey>,
}

pub struct WalletCriteria;

impl Criteria for WalletCriteria {
    fn criteria_fulfilled(&self, hash: &[u8]) -> bool {
        true
    }
}

pub struct WalletValidator;

impl Validate<Wallet> for WalletValidator {
    fn block_valid(&self, block: &BlockCandidate<Wallet>) -> Result<(), Box<dyn BlockchainError>> {
        todo!()
    }
}

impl Wallet {
    pub fn new(address: Address, public_key: Option<RsaPublicKey>) -> Wallet {
        Wallet {
            address,
            public_key,
        }
    }
    pub fn address(&self) -> [u8; 32] {
        self.address
    }
    pub fn key(&self) -> &Option<RsaPublicKey> {
        &self.public_key
    }

    pub fn balance(
        &self, transaction_chain: &Blockchain<Transaction>,
    ) -> i64 {
        if self.address == MINTING_WALLET_ADDRESS {
            return transaction_chain.remaining_pool();
        }
        let mut current_block = transaction_chain.last_block();
        let mut balance: i64 = 0;
        loop {
            match current_block {
                None => break,
                Some(block) => {
                    balance += self.balance_pool(block.data());
                    current_block = block.previous_block();
                }
            }
        }
        balance += self.balance_pool(transaction_chain.uncommitted_data());
        balance
    }

    fn balance_pool(&self, transaction_pool: &[Transaction]) -> i64 {
        let mut spent = 0;
        let mut gained = 0;
        for transaction in transaction_pool {
            if transaction.source_address == self.address {
                spent += transaction.amount;
            } else if transaction.target_address == self.address {
                gained += transaction.amount;
            }
        }
        gained - spent
    }
}

impl Summary for Wallet {
    fn summary(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

impl BlockchainData for Wallet {}

struct BalanceError;

impl BlockchainError for BalanceError {
    fn message(&self) -> String {
        String::from("Illegal balance")
    }
}

struct TransactionValidationError;

impl BlockchainError for TransactionValidationError {
    fn message(&self) -> String {
        String::from("Transaction invalid")
    }
}

pub fn find_wallet_by_address(address: Address, wallet_chain: &Blockchain<Wallet>) -> Option<Wallet> {
    let mut current_block = wallet_chain.last_block();
    loop {
        match current_block {
            None => break None,
            Some(block) => {
                match extract_wallet(block.data(), address) {
                    None => current_block = block.previous_block(),
                    Some(wallet) => break Some(wallet)
                }
            }
        }
    }
}

fn extract_wallet(data: &Vec<Wallet>, address: Address) -> Option<Wallet> {
    for entry in data {
        if entry.address() == address {
            return Some(entry.clone());
        }
    };
    None
}

mod test {
    use std::cell::RefCell;

    use chrono::Utc;
    use rsa::{RsaPrivateKey, RsaPublicKey};
    use rsa::pss::BlindedSigningKey;
    use rsa::rand_core::{CryptoRng, RngCore};
    use rsa::signature::RandomizedSigner;
    use serde::Serialize;
    use sha2::Sha512;

    use crate::blockchain::{BlockchainData, MINTING_WALLET_ADDRESS, Transaction, TRANSACTION_FEE, TransactionCriteria, TransactionValidator, Wallet, WalletCriteria, WalletValidator};
    use crate::blockchain::core::{Block, BlockCandidate, Blockchain, BlockchainError, BlockKey, BlockPointer, Summary, Validate};
    use crate::BlockHash;

    #[test]
    fn ok_on_valid_transaction() {
        let mut rng = rand::thread_rng();

        let mut wallets = Blockchain::<Wallet>::wallet_chain();
        let first_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let second_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let third_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let new_wallets = prepare_wallets_block(
            wallets.last_block(), &first_key,
            &second_key, &third_key,
        );

        wallets.submit_new_block(new_wallets);

        let minted: i64 = 70;
        let transaction_amount = 5;
        let transactions = Blockchain::<Transaction>::transaction_chain(
            vec![
                Transaction::new(
                    MINTING_WALLET_ADDRESS,
                    [1; 32],
                    "Transaction".to_string(), minted, Utc::now(),
                )
            ]
        );
        let mut transaction = Transaction::new(
            [1; 32],
            [2; 32],
            "Transaction".to_string(), transaction_amount, Utc::now(),
        );
        let reward = Transaction::new(
            MINTING_WALLET_ADDRESS,
            [3; 32],
            "Reward".to_string(), TRANSACTION_FEE, Utc::now(),
        );
        transaction.sign(BlindedSigningKey::<Sha512>::new(first_key), rng);

        let to_validate = vec![transaction, reward];
        let block_candidate = prepare_block_candidate(
            transactions.last_block(), to_validate,
        );

        let validator = TransactionValidator {
            wallets: &wallets,
            transactions: &transactions,
        };
        match validator.block_valid(&block_candidate) {
            Ok(_) => {
                println!("success");
            }
            Err(err) => {
                panic!("validation failed: {}", err.message());
            }
        }
    }

    fn prepare_wallets_block(
        previous_block: &BlockPointer<Wallet>, first_key: &RsaPrivateKey,
        second_key: &RsaPrivateKey, third_key: &RsaPrivateKey,
    ) -> BlockCandidate<Wallet> {
        let wallets = vec![
            Wallet {
                address: [1; 32],
                public_key: Some(RsaPublicKey::from(first_key)),
            }, Wallet {
                address: [2; 32],
                public_key: Some(RsaPublicKey::from(second_key)),
            }, Wallet {
                address: [3; 32],
                public_key: Some(RsaPublicKey::from(third_key)),
            },
        ];
        prepare_block_candidate(previous_block, wallets)
    }

    fn prepare_block_candidate<T>(
        previous_block: &BlockPointer<T>, data: Vec<T>,
    ) -> BlockCandidate<T> where T: BlockchainData {
        match BlockCandidate::create_new(data, previous_block) {
            Ok(block_candidate) => block_candidate,
            Err(error) => panic!("{}", error.message())
        }
    }
}