use sha2::{Sha512, Digest};
use crate::blockchain::{Address};

pub struct Credentials {
    user_key: String,
    password: String,
    salt: String,
    wallet_address: Address
}

pub fn fetch_wallet_address(credentials: Credentials) -> Address {
    let mut hasher = Sha512::new();
    hasher.update(&credentials.user_key);
    hasher.update(&credentials.password);
    hasher.update(&credentials.salt);
    let key: String = array_bytes::bytes2hex(
        "", hasher.finalize().as_slice(),
    );
    todo!()
}
//TODO credentials:
// - generify blockchain implementation
// - encrypt and store credentials on a separate chain
// - encrypt wallet
// - implement asymmetric encryption for wallet (public/private key)