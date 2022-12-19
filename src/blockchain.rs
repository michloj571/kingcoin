
pub mod core;
mod access;

pub const WALLET_ADDRESS_SIZE: usize = 32;

pub trait BlockchainData {
}

pub struct Address {
    value: [u8; WALLET_ADDRESS_SIZE],
}

pub struct Wallet {
    address: Address,
    balance: u64,
    key: String,
}

impl Address {
    fn new(value: [u8; 32]) -> Address {
        Address {
            value
        }
    }
}

impl AsRef<[u8]> for Address {
    fn as_ref(&self) -> &[u8] {
        &self.value
    }
}

impl Clone for Address {
    fn clone(&self) -> Self {
        Address {
            value: self.value
        }
    }
}

impl Copy for Address {}

impl Into<String> for Address {
    fn into(self) -> String {
        array_bytes::bytes2hex("", self.value)
    }
}

impl TryFrom<String> for Address {
    type Error = ();

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let result = array_bytes::hex2array(value);
        match result {
            Ok(value) => {
                Ok(Address {
                    value
                })
            }
            Err(_) => {
                Err(())
            }
        }
    }
}

//todo implement blockchain's functional API:
// - searching the blockchain
// - validation of peers' block
// - unconfirmed transactions management