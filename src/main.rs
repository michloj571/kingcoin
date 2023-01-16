use std::error::Error;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use io::BufReader;
use libp2p::{futures::StreamExt, Swarm};
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde::{Deserialize, Serialize};
use tokio::io::{self, AsyncBufReadExt};

use kingcoin::{
    blockchain::{core::Blockchain, Transaction, Wallet},
    network::{self, communication::dispatch, NodeState},
};
use kingcoin::blockchain::{Address, HotWallet};
use kingcoin::network::BlockchainBehaviour;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut swarm = network::configure_swarm();
    println!("This node id: {}", swarm.local_peer_id());
    let (
        mut node_state,
        mut transactions,
        mut wallets,
        mut stakes
    ) = initialize_node(&mut swarm);
    let mut stdin = BufReader::new(io::stdin()).lines();
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
    loop {
        tokio::select! {
            io_result = stdin.next_line() => {
                match io_result {
                    Ok(command) => {
                        let stop = !dispatch_command(command);
                        if stop {
                            break Ok(());
                        }
                    },
                    Err(error) => println!("{}", error.to_string())
                }
            },
            event = swarm.select_next_some() => {
                dispatch::dispatch_network_event(
                    event, &mut swarm, &mut transactions,
                    &mut wallets, &mut node_state, &mut stakes
                );
            }
        }
    }
}

fn initialize_node(
    swarm: &mut Swarm<BlockchainBehaviour>
) -> (NodeState, Blockchain<Transaction>, Blockchain<Wallet>, Blockchain<Transaction>) {
    let path = Path::new("wallet");
    let user_wallet = match File::open(&path) {
        Err(_) => {
            new_wallet()
        }
        Ok(mut file) => {
            read_wallet(&mut file)
        }
    };
    let mut stakes = Blockchain::<Transaction>::transaction_chain(
        vec![],
    );
    let mut wallets = Blockchain::<Wallet>::wallet_chain();
    let mut transactions = Blockchain::<Transaction>::transaction_chain(
        vec![]
    );
    let mut node_state = NodeState::init(
        swarm.local_peer_id().clone(), user_wallet, &transactions, &stakes,
    );
    (node_state, transactions, wallets, stakes)
}

fn dispatch_command(command: Option<String>) -> bool {
    todo!()
}

fn read_wallet(file: &mut File) -> HotWallet {
    let mut buffer = String::new();
    match file.read_to_string(&mut buffer) {
        Err(_) => {
            new_wallet()
        }
        Ok(_) => {
            serde_json::from_str(&buffer).unwrap()
        }
    }
}

fn new_wallet() -> HotWallet {
    println!("Could not find wallet, generating new");
    let private_key = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
    let wallet = HotWallet::generate(private_key);
    println!("Wallet address {}", array_bytes::bytes2hex("", wallet.address()));

    wallet
}
