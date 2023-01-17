use std::collections::HashSet;
use std::error::Error;
use std::os::macos::raw::stat;
use std::str::FromStr;

use chrono::Utc;
use io::BufReader;
use libp2p::{futures::StreamExt, Swarm};
use libp2p::gossipsub::GossipsubEvent;
use libp2p::swarm::SwarmEvent;
use rsa::pss::BlindedSigningKey;
use rsa::RsaPrivateKey;
use tokio::io::{self, AsyncBufReadExt};

use kingcoin::{blockchain::{core::Blockchain, Transaction, Wallet}, blockchain, network::{self, communication::dispatch, NodeState}};
use kingcoin::blockchain::{Address, HotWallet, StakeBid};
use kingcoin::network::{BlockchainBehaviour, BlockchainBehaviourEvent, communication};
use kingcoin::network::communication::{BlockchainDto, BlockchainMessage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut swarm = network::configure_swarm();
    println!("This node id: {}", swarm.local_peer_id());
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
    let (
        mut node_state,
        mut transactions,
        mut stakes
    ) = initialize_node(&mut swarm).await;
    println!(
        "This node wallet:{}",
        array_bytes::bytes2hex("", node_state.user_wallet().address())
    );
    get_allowance(&mut swarm, &mut node_state, &mut transactions).await;
    let mut stdin = BufReader::new(io::stdin()).lines();
    loop {
        tokio::select! {
            io_result = stdin.next_line() => {
                match io_result {
                    Ok(command) => {
                        let stop = !dispatch_command(
                            command, &mut node_state,
                            &mut transactions, &mut stakes, &mut swarm
                        );
                        if stop {
                            break Ok(());
                        }
                    },
                    Err(error) => println!("{}", error.to_string())
                }
            },
            event = swarm.select_next_some() => {
                dispatch::dispatch_network_event(
                    event, &mut swarm, &mut transactions, &mut node_state, &mut stakes
                );
            }
        }
    }
}

fn dispatch_command(
    command: Option<String>,
    node_state: &mut NodeState,
    transactions: &mut Blockchain<Transaction>,
    stakes: &mut Blockchain<Transaction>,
    swarm: &mut Swarm<BlockchainBehaviour>,
) -> bool {
    match command {
        None => true,
        Some(command) => {
            parse(command, node_state, transactions, stakes, swarm)
        }
    }
}

fn parse(
    command: String,
    node_state: &mut NodeState,
    transactions: &mut Blockchain<Transaction>,
    stakes: &mut Blockchain<Transaction>,
    swarm: &mut Swarm<BlockchainBehaviour>,
) -> bool {
    let tokens = command.split(' ').collect::<Vec<&str>>();
    let supported_commands = HashSet::from([
        "send", "list", "exit", "balance"
    ]);
    match tokens.len() {
        1 => {
            let token = tokens.get(0).unwrap();
            if token.eq_ignore_ascii_case("exit") {
                false
            } else if token.eq_ignore_ascii_case("list") {
                list_transactions(node_state.user_wallet(), transactions);
                true
            } else if token.eq_ignore_ascii_case("balance") {
                let balance = node_state.user_wallet().to_wallet().balance(
                    transactions, stakes,
                );
                println!("Your balance: {balance}KGC\n");
                true
            } else {
                println!("invalid command");
                true
            }
        }
        3 => {
            let token = tokens.get(0).unwrap();
            if supported_commands.contains(token) {
                let decimal_amount = tokens.get(1).unwrap();
                let amount = i64::from_str(decimal_amount);
                let amount = match amount {
                    Ok(amount) => amount,
                    Err(_) => {
                        println!("Not a number");
                        return true;
                    }
                };
                let wallet = node_state.user_wallet();
                let balance = wallet.to_wallet().balance(transactions, stakes);
                let required_balance = amount + blockchain::TRANSACTION_FEE;
                if balance >= required_balance {
                    let source_address = wallet.address();
                    let target_address = tokens.get(2).unwrap();
                    let target_address: Address = array_bytes::hex2array(target_address).unwrap();

                    let mut rng = rand::thread_rng();

                    let mut transaction = Transaction::new(
                        source_address, target_address,
                        "".to_string(), amount, Utc::now(),
                    );
                    let mut transaction_fee = Transaction::new(
                        source_address, *blockchain::REWARD_WALLET_ADDRESS,
                        "".to_string(), blockchain::TRANSACTION_FEE, Utc::now(),
                    );
                    transaction.sign(
                        BlindedSigningKey::from(
                            wallet.private_key().clone()
                        ), &mut rng,
                    );
                    transaction_fee.sign(
                        BlindedSigningKey::from(
                            wallet.private_key().clone()
                        ), &mut rng,
                    );
                    transactions.add_uncommitted(transaction.clone());
                    transactions.add_uncommitted(transaction_fee.clone());
                    if transactions.has_enough_uncommitted_data() {
                        communication::publish_message(
                            swarm, BlockchainMessage::SubmitTransaction {
                                transaction, transaction_fee
                            }
                        );
                        let bid = node_state.user_wallet()
                            .to_wallet()
                            .balance(transactions, stakes) * 75 / 100;
                        node_state.update_bid(StakeBid::bid(bid, node_state.user_wallet().address()));
                        communication::publish_message(
                            swarm,
                            BlockchainMessage::Bid(node_state.node_bid().unwrap()),
                        );
                    }
                } else {
                    println!(
                        "Balance to low. Your balance: {}KGC, required: {}KGC",
                        balance, required_balance
                    );
                    return true;
                }
                true
            } else {
                println!("Unsupported command");
                true
            }
        }
        _ => true
    }
}

fn submit_transaction(
    transactions: &mut Blockchain<Transaction>,
    swarm: &mut Swarm<BlockchainBehaviour>,
    tokens: Vec<&str>, amount: i64, wallet: &HotWallet,
    node_state: &mut NodeState, stakes: &mut Blockchain<Transaction>,
) {

}

fn list_transactions(wallet: &HotWallet, transactions: &Blockchain<Transaction>) {
    let mut current_block = transactions.last_block();
    println!("Your transactions:");
    loop {
        match current_block {
            None => break,
            Some(block) => {
                block.data()
                    .iter()
                    .filter(|transaction| {
                        transaction.source_address() == wallet.address() ||
                            transaction.target_address() == wallet.address()
                    })
                    .for_each(|transaction| {
                        let transaction = serde_json::to_string(transaction).unwrap();
                        println!("{transaction}");
                    });
                current_block = block.previous_block();
            }
        }
    }
    println!();
}

async fn get_allowance(
    swarm: &mut Swarm<BlockchainBehaviour>,
    node_state: &mut NodeState, transactions: &mut Blockchain<Transaction>) {
    let mut granted = false;
    let wallet = node_state.user_wallet().to_wallet();
    while !granted {
        communication::publish_message(
            swarm, BlockchainMessage::GrantAllowance(wallet.clone())
        );
        tokio::select! {
            event = swarm.select_next_some() => {
                granted = request_allowance(event, transactions, swarm, node_state);
            }
        }
    }
}

fn request_allowance<H>(
    event: SwarmEvent<BlockchainBehaviourEvent, H>,
    transactions: &mut Blockchain<Transaction>,
    swarm: &mut Swarm<BlockchainBehaviour>, node_state: &mut NodeState,
) -> bool {
    match event {
        SwarmEvent::Behaviour(BlockchainBehaviourEvent::Mdns(event)) => {
            dispatch::dispatch_mdns(swarm, event, node_state);
            false
        }
        SwarmEvent::Behaviour(BlockchainBehaviourEvent::Gossipsub(
                                  GossipsubEvent::Message {
                                      propagation_source: peer_id,
                                      message_id: _id,
                                      message,
                                  })
        ) => {
            if let Ok(message) = serde_json::from_slice::<BlockchainMessage>(&message.data) {
                match message {
                    BlockchainMessage::Granted(amount) => {
                        dispatch::assign_allowance(
                            transactions,
                            &node_state.user_wallet().to_wallet(),
                            amount,
                        );
                        true
                    }
                    BlockchainMessage::GrantAllowance(wallet) => {
                        if node_state.add_peer_wallet(peer_id, wallet.clone()).is_none() {
                            let amount = transactions.mint(1000);

                            dispatch::assign_allowance(transactions, &wallet, amount);

                            communication::publish_message(
                                swarm, BlockchainMessage::Granted(amount),
                            );
                        }
                        false
                    }
                    _ => false
                }
            } else {
                false
            }
        }
        _ => false
    }
}

async fn initialize_node(
    swarm: &mut Swarm<BlockchainBehaviour>
) -> (NodeState, Blockchain<Transaction>, Blockchain<Transaction>) {
    let private_key = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
    let user_wallet = HotWallet::generate(private_key);
    let mut stakes = Blockchain::<Transaction>::transaction_chain(
        vec![],
    );
    let mut transactions = Blockchain::<Transaction>::transaction_chain(
        vec![]
    );
    let mut node_state = NodeState::init(
        swarm.local_peer_id().clone(), user_wallet,
    );

    connect_to_network(swarm, &mut node_state).await;
    sync_peer(
        swarm, &mut transactions,
        &mut stakes, &mut node_state,
    ).await;

    (node_state, transactions, stakes)
}

async fn connect_to_network(swarm: &mut Swarm<BlockchainBehaviour>, node_state: &mut NodeState) {
    let mut connected = false;
    while !connected {
        tokio::select! {
            event = swarm.select_next_some() => {
                connected = find_peer(swarm, event, node_state);
            }
        }
    }
}

async fn sync_peer(
    swarm: &mut Swarm<BlockchainBehaviour>,
    transactions: &mut Blockchain<Transaction>,
    stakes: &mut Blockchain<Transaction>,
    node_state: &mut NodeState,
) {
    let mut subscribed = false;
    while !subscribed {
        tokio::select! {
            event = swarm.select_next_some() => {
                subscribed = joined_on_subscribed(
                    swarm, node_state, node_state.user_wallet().to_wallet(),event
                )
            }
        }
    }

    let mut synced = false;
    while !synced {
        tokio::select! {
            event = swarm.select_next_some() => {
                synced = handled_sync_packet(
                    transactions, stakes, node_state, swarm, event
                )
            }
        }
    }
}

fn find_peer<H>(
    swarm: &mut Swarm<BlockchainBehaviour>,
    event: SwarmEvent<BlockchainBehaviourEvent, H>,
    node_state: &mut NodeState,
) -> bool {
    match event {
        SwarmEvent::Behaviour(BlockchainBehaviourEvent::Mdns(event)) => {
            dispatch::dispatch_mdns(swarm, event, node_state);
            true
        }
        _ => false
    }
}

fn joined_on_subscribed<H>(
    swarm: &mut Swarm<BlockchainBehaviour>, node_state: &mut NodeState,
    wallet: Wallet, event: SwarmEvent<BlockchainBehaviourEvent, H>,
) -> bool {
    match event {
        SwarmEvent::Behaviour(BlockchainBehaviourEvent::Mdns(event)) => {
            dispatch::dispatch_mdns(swarm, event, node_state);
            false
        }
        SwarmEvent::Behaviour(BlockchainBehaviourEvent::Gossipsub(event)) => {
            match event {
                GossipsubEvent::Subscribed { .. } => {
                    communication::publish_message(
                        swarm, BlockchainMessage::Join(wallet),
                    );
                    true
                }
                _ => false
            }
        }
        _ => false
    }
}

fn handled_sync_packet<H>(
    transactions: &mut Blockchain<Transaction>,
    stakes: &mut Blockchain<Transaction>,
    node_state: &mut NodeState, swarm: &mut Swarm<BlockchainBehaviour>,
    event: SwarmEvent<BlockchainBehaviourEvent, H>,
) -> bool {
    match event {
        SwarmEvent::Behaviour(BlockchainBehaviourEvent::Mdns(event)) => {
            dispatch::dispatch_mdns(swarm, event, node_state);
            false
        }
        SwarmEvent::Behaviour(BlockchainBehaviourEvent::Gossipsub(
                                  GossipsubEvent::Message {
                                      propagation_source: peer_id,
                                      message_id: _id,
                                      message,
                                  })
        ) => {
            if let Ok(message) = serde_json::from_slice::<BlockchainMessage>(&message.data) {
                match message {
                    BlockchainMessage::Sync {
                        transactions: received_transactions,
                        wallets,
                        stakes: received_stakes
                    } => {
                        if received_transactions.chain_length() > transactions.chain_length() {
                            *transactions = Blockchain::from(received_transactions);
                        }

                        if received_stakes.chain_length() > stakes.chain_length() {
                            *stakes = Blockchain::from(received_stakes)
                        }

                        node_state.add_wallets(wallets);

                        true
                    }
                    BlockchainMessage::Join(_) => {
                        communication::publish_message(
                            swarm, BlockchainMessage::Sync {
                                transactions: BlockchainDto::from(transactions),
                                wallets: node_state.wallets().clone(),
                                stakes: BlockchainDto::from(stakes),
                            },
                        );
                        false
                    }
                    _ => false
                }
            } else {
                false
            }
        }
        _ => false
    }
}