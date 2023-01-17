use std::error::Error;

use io::BufReader;
use libp2p::{futures::StreamExt, Swarm};
use libp2p::gossipsub::GossipsubEvent;
use libp2p::swarm::SwarmEvent;

use rsa::{RsaPrivateKey};
use tokio::io::{self, AsyncBufReadExt};

use kingcoin::{
    blockchain::{core::Blockchain, Transaction, Wallet},
    network::{self, communication::dispatch, NodeState},
};
use kingcoin::blockchain::{HotWallet};
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
    let mut stdin = BufReader::new(io::stdin()).lines();
    println!("listening");
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
                    event, &mut swarm, &mut transactions, &mut node_state, &mut stakes
                );
            }
        }
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

    await_connection(swarm).await;
    sync_peer(
        swarm, &mut transactions,
        &mut stakes, &mut node_state
    ).await;

    (node_state, transactions, stakes)
}

async fn await_connection(swarm: &mut Swarm<BlockchainBehaviour>) {
    let mut connected = false;
    while !connected {
        tokio::select! {
            event = swarm.select_next_some() => {
                connected = find_peer(swarm, event);
            }
        }
    }
}

async fn sync_peer(
    swarm: &mut Swarm<BlockchainBehaviour>,
    transactions: &mut Blockchain<Transaction>,
    stakes: &mut Blockchain<Transaction>,
    node_state: &mut NodeState
) {
    let mut subscribed = false;
    while !subscribed {
        tokio::select! {
            event = swarm.select_next_some() => {
                subscribed = joined_on_subscribed(
                    swarm, node_state.user_wallet().to_wallet(), event
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
) -> bool {
    match event {
        SwarmEvent::Behaviour(BlockchainBehaviourEvent::Mdns(event)) => {
            dispatch::dispatch_mdns(swarm, event);
            true
        }
        _ => false
    }
}

fn joined_on_subscribed<H>(
    swarm: &mut Swarm<BlockchainBehaviour>,
    wallet: Wallet, event: SwarmEvent<BlockchainBehaviourEvent, H>,
) -> bool {
    match event {
        SwarmEvent::Behaviour(BlockchainBehaviourEvent::Gossipsub(event)) => {
            match event {
                GossipsubEvent::Subscribed {..} => {
                    communication::publish_message(
                        swarm, BlockchainMessage::Join(wallet)
                    );
                    true
                },
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
    event: SwarmEvent<BlockchainBehaviourEvent, H>
) -> bool {
    match event {
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

                        if received_stakes .chain_length() > stakes.chain_length() {
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
                            }
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

fn dispatch_command(command: Option<String>) -> bool {
    todo!()
}