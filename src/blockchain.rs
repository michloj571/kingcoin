use std::collections::HashSet;

use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use rsa::{pss::VerifyingKey, RsaPrivateKey, RsaPublicKey, signature::{Signature, Verifier}};
use rsa::pss::BlindedSigningKey;
use rsa::rand_core::{CryptoRng, RngCore};
use rsa::signature::RandomizedSigner;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};

use crate::blockchain::core::{BlockCandidate, Blockchain, BlockchainError, BlockPointer, BlockValidationError, Summary, Validate};
use crate::network::NodeState;

pub mod core;

pub type Address = [u8; 32];

pub static TRANSACTION_FEE: i64 = 50;
pub static TRANSACTIONS_PER_BLOCK: u64 = 1;
pub static MINTING_WALLET_ADDRESS: Address = [0; 32];
lazy_static! {
    pub static ref STAKE_WALLET_ADDRESS: Address = {
        let mut address = [0;32];
        address[0] = 1;
        address
    };
    pub static ref REWARD_WALLET_ADDRESS: Address = {
        let mut address = [0;32];
        address[0] = 2;
        address
    };
    pub static ref TOTAL_TRANSFERS_PER_BLOCK: u64 = 2 * TRANSACTIONS_PER_BLOCK + 2;
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

    pub fn mint(allowance: i64, target_address: Address) -> Transaction {
        Transaction::new(
            MINTING_WALLET_ADDRESS, target_address,
            "MINT".to_string(), allowance, Utc::now()
        )
    }

    pub fn stake_return(bid: i64, target_address: Address) -> Transaction {
        Transaction::new(
            *STAKE_WALLET_ADDRESS, target_address, "".to_string(),
            bid, Utc::now(),
        )
    }

    pub fn forging_reward(target_address: Address) -> Transaction {
        Transaction::new(
            *REWARD_WALLET_ADDRESS, target_address, "".to_string(),
            TRANSACTION_FEE * TRANSACTIONS_PER_BLOCK as i64, Utc::now(),
        )
    }
}

#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone)]
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
    wallets: &'a HashSet<Wallet>,
    transactions: &'a Blockchain<Transaction>,
    stakes: &'a Blockchain<Transaction>,
}

impl<'a> Validate<Transaction> for TransactionValidator<'a> {
    fn block_valid(&self, block: &BlockCandidate<Transaction>) -> Result<(), Box<dyn BlockchainError>> {
        if block.data().len() == *TOTAL_TRANSFERS_PER_BLOCK as usize {
            self.validate_hash(block)?;
            self.validate_stake_return(block)?;
            self.validate_reward_transfer(block)?;
            self.validate_transactions(block)
        } else {
            Err(
                Box::new(TransactionValidationError)
            )
        }
    }
}

impl<'a> TransactionValidator<'a> {
    pub fn new(
        wallets: &'a HashSet<Wallet>,
        stakes: &'a Blockchain<Transaction>,
        transactions: &'a Blockchain<Transaction>,
    ) -> TransactionValidator<'a> {
        Self {
            wallets,
            stakes,
            transactions,
        }
    }

    fn is_not_special(transaction: &Transaction) -> bool {
        transaction.source_address() != *REWARD_WALLET_ADDRESS &&
            transaction.source_address() != *STAKE_WALLET_ADDRESS &&
            transaction.target_address() != *REWARD_WALLET_ADDRESS
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

    fn validate_transactions(&self, block: &BlockCandidate<Transaction>) -> Result<(), Box<dyn BlockchainError>> {
        block.data()
            .iter()
            .filter(|transaction| TransactionValidator::is_not_special(transaction))
            .try_for_each(|transaction| self.validate_transaction(transaction))
    }

    pub fn validate_transaction(
        &self, transaction: &Transaction,
    ) -> Result<(), Box<dyn BlockchainError>> {
        if transaction.source_address() == transaction.target_address() {
            return Err(
                Box::new(TransactionValidationError)
            );
        }

        let signature = match transaction.sender_signature() {
            None => return Err(
                Box::new(TransactionValidationError)
            ),
            Some(signature) => signature
        };

        let source_wallet = find_wallet_by_address(
            self.wallets, transaction.source_address(),
        );

        match find_wallet_by_address(&self.wallets, transaction.target_address()) {
            None => return Err(
                Box::new(TransactionValidationError)
            ),
            Some(wallet) => wallet
        };

        match source_wallet {
            None => Err(
                Box::new(TransactionValidationError)
            ),
            Some(wallet) => {
                self.validate_transfer(transaction, signature, wallet)
            }
        }
    }

    fn validate_stake_return(&self, block: &BlockCandidate<Transaction>) -> Result<(), Box<dyn BlockchainError>> {
        let stake_bid = match self.stakes.last_block() {
            None => return Err(
                Box::new(TransactionValidationError)
            ),
            Some(block) => {
                &block.data()[0]
            }
        };
        let stake_returns = block.data()
            .iter()
            .filter(|transaction| transaction.source_address() == *STAKE_WALLET_ADDRESS)
            .filter(|transaction| transaction.target_address() == stake_bid.source_address())
            .filter(|transaction| transaction.amount() == stake_bid.amount())
            .count();
        if stake_returns == 1 {
            Ok(())
        } else {
            Err(
                Box::new(TransactionValidationError)
            )
        }
    }

    fn validate_reward_transfer(&self, block: &BlockCandidate<Transaction>) -> Result<(), Box<dyn BlockchainError>> {
        match self.stakes.last_block() {
            None => Err(
                Box::new(TransactionValidationError)
            ),
            Some(stakes_block) => {
                let total_fee: i64 = block.data()
                    .iter()
                    .filter(|transaction| transaction.target_address() == *REWARD_WALLET_ADDRESS)
                    .filter(|transaction| transaction.amount() == TRANSACTION_FEE)
                    .map(|transaction| transaction.amount())
                    .sum();
                let expected_reward = TRANSACTION_FEE * TRANSACTIONS_PER_BLOCK as i64;
                let reward: i64 = block.data()
                    .iter()
                    .filter(|transaction| transaction.source_address() == *REWARD_WALLET_ADDRESS)
                    .filter(|transaction| transaction.target_address() == stakes_block.data()[0].source_address())
                    .map(|transaction| transaction.amount())
                    .sum();
                if reward == total_fee && total_fee == expected_reward {
                    Ok(())
                } else {
                    Err(
                        Box::new(TransactionValidationError)
                    )
                }
            }
        }
    }

    fn validate_transfer(&self, transaction: &Transaction, signature: &str, source_wallet: Wallet) -> Result<(), Box<dyn BlockchainError>> {
        let available_balance = source_wallet.balance(
            self.transactions, self.stakes,
        );
        let verified = source_wallet.is_signature_valid(transaction, signature);
        if !verified || available_balance < transaction.amount {
            Err(
                Box::new(TransactionValidationError)
            )
        } else {
            Ok(())
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Wallet {
    address: [u8; 32],
    public_key: Option<RsaPublicKey>,
}


#[derive(Serialize, Deserialize)]
pub struct HotWallet {
    address: Address,
    private_key: RsaPrivateKey,
}

impl HotWallet {
    fn new(address: Address, private_key: RsaPrivateKey) -> HotWallet {
        HotWallet {
            address,
            private_key,
        }
    }

    pub fn generate(private_key: RsaPrivateKey) -> HotWallet {
        let mut hasher = Sha256::new();
        hasher.update(Utc::now().to_string());
        let wallet_address: Address = hasher.finalize()
            .try_into()
            .expect("Error creating wallet");

        HotWallet::new(
            wallet_address, private_key,
        )
    }

    pub fn to_wallet(&self) -> Wallet {
        let public_key = RsaPublicKey::from(&self.private_key);
        Wallet::new(
            self.address, Some(public_key),
        )
    }

    pub fn address(&self) -> Address {
        self.address
    }

    pub fn private_key(&self) -> &RsaPrivateKey {
        &self.private_key
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

    pub fn is_signature_valid(&self, transaction: &Transaction, signature: &str) -> bool {
        let public_key = match self.key() {
            None => return false,
            Some(key) => key.clone(),
        };
        let key: VerifyingKey<Sha512> = VerifyingKey::from(public_key);
        let verified = key.verify(
            transaction.signed_content().as_bytes(),
            &Signature::from_bytes(signature.as_bytes()).unwrap())
            .is_err();

        verified
    }


    pub fn balance(
        &self, transaction_chain: &Blockchain<Transaction>,
        stakes_chain: &Blockchain<Transaction>,
    ) -> i64 {
        if self.address == MINTING_WALLET_ADDRESS {
            return transaction_chain.remaining_pool();
        }
        let mut balance = self.balance_chain(transaction_chain);
        balance += self.balance_pool(transaction_chain.uncommitted_data());
        balance += self.balance_chain(stakes_chain);
        balance
    }

    fn balance_chain(&self, blockchain: &Blockchain<Transaction>) -> i64 {
        let mut balance: i64 = 0;
        let mut current_block = blockchain.last_block();
        loop {
            match current_block {
                None => break,
                Some(block) => {
                    balance += self.balance_pool(block.data());
                    current_block = block.previous_block();
                }
            }
        }
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

pub fn find_wallet_by_address(wallets: &HashSet<Wallet>, address: Address) -> Option<Wallet> {
    wallets.iter()
        .find(|wallet| wallet.address() == address)
        .map(|wallet| wallet.clone())
}

impl Summary for Wallet {
    fn summary(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

impl BlockchainData for Wallet {}

#[derive(Debug)]
struct BalanceError;

impl BlockchainError for BalanceError {
    fn message(&self) -> String {
        String::from("Illegal balance")
    }
}

#[derive(Debug)]
struct TransactionValidationError;

impl BlockchainError for TransactionValidationError {
    fn message(&self) -> String {
        String::from("Transaction invalid")
    }
}

mod test {
    use std::collections::HashSet;

    use chrono::Utc;
    use rsa::{RsaPrivateKey, RsaPublicKey};
    use rsa::pss::BlindedSigningKey;
    use rsa::rand_core::{CryptoRng, RngCore};
    use rsa::signature::RandomizedSigner;
    use serde::Serialize;
    use sha2::Sha512;

    use crate::blockchain::{BlockchainData, MINTING_WALLET_ADDRESS, REWARD_WALLET_ADDRESS, STAKE_WALLET_ADDRESS, Transaction, TRANSACTION_FEE, TransactionValidator, Wallet};
    use crate::blockchain::core::{Block, BlockCandidate, Blockchain, BlockchainError, BlockKey, BlockPointer, Summary, Validate};
    use crate::BlockHash;

    #[test]
    fn ok_on_valid_transaction() {
        let mut rng = rand::thread_rng();

        let first_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let second_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let third_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let wallet_set = prepare_wallets_hashset(&first_key, &second_key, &third_key);

        let minted: i64 = 70;
        let transaction_amount = 5;
        let transactions = Blockchain::<Transaction>::transaction_chain(
            vec![
                Transaction::new(
                    MINTING_WALLET_ADDRESS,
                    [1; 32],
                    "Transaction".to_string(), minted, Utc::now(),
                ),
                Transaction::new(
                    MINTING_WALLET_ADDRESS,
                    [3; 32],
                    "Transaction".to_string(), minted, Utc::now(),
                ),
            ]
        );
        let bid = 10;
        let stakes = Blockchain::<Transaction>::transaction_chain(
            vec![
                Transaction::new(
                    [3; 32],
                    *STAKE_WALLET_ADDRESS,
                    "Transaction".to_string(), bid, Utc::now(),
                )
            ]
        );
        let mut transaction = Transaction::new(
            [1; 32],
            [2; 32],
            "Transaction".to_string(), transaction_amount, Utc::now(),
        );
        transaction.sign(BlindedSigningKey::<Sha512>::new(first_key.clone()), rng.clone());
        let mut transaction_fee = Transaction::new(
            [1; 32],
            *REWARD_WALLET_ADDRESS,
            "Transaction".to_string(), TRANSACTION_FEE, Utc::now(),
        );
        transaction_fee.sign(BlindedSigningKey::<Sha512>::new(first_key), rng);
        let reward = Transaction::new(
            *REWARD_WALLET_ADDRESS,
            [3; 32],
            "Reward".to_string(), TRANSACTION_FEE, Utc::now(),
        );
        let stake_return = Transaction::stake_return(
            bid, [3; 32],
        );

        let to_validate = vec![transaction, transaction_fee, stake_return, reward];
        let block_candidate = prepare_block_candidate(
            transactions.last_block(), to_validate,
        );

        let validator = TransactionValidator {
            wallets: &wallet_set,
            transactions: &transactions,
            stakes: &stakes,
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

    fn prepare_wallets_hashset(first_key: &RsaPrivateKey,
                               second_key: &RsaPrivateKey, third_key: &RsaPrivateKey,
    ) -> HashSet<Wallet> {
        let mut wallets: HashSet<Wallet> = HashSet::new();
        wallets.insert(Wallet {
            address: [1; 32],
            public_key: Some(RsaPublicKey::from(first_key)),
        });
        wallets.insert(Wallet {
            address: [2; 32],
            public_key: Some(RsaPublicKey::from(second_key)),
        });
        wallets.insert(Wallet {
            address: [3; 32],
            public_key: Some(RsaPublicKey::from(third_key)),
        });
        wallets
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