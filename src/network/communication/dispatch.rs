use std::collections::HashSet;

use libp2p::{PeerId, Swarm};
use libp2p::gossipsub::GossipsubEvent;
use libp2p::mdns::Event;
use libp2p::swarm::SwarmEvent;

use crate::blockchain::{BlockchainData, StakeBid, Transaction, TransactionValidator, Wallet};
use crate::blockchain::core::{BlockCandidate, Blockchain, BlockchainError, TransactionCountError, Validate};
use crate::network::{BlockchainBehaviour, BlockchainBehaviourEvent, communication::{self, BlockDto, Vote}, NodeState};
use crate::network::communication::BlockchainDto;

use super::BlockchainMessage;

pub fn dispatch_network_event<H>(
    event: SwarmEvent<BlockchainBehaviourEvent, H>, swarm: &mut Swarm<BlockchainBehaviour>,
    transactions: &mut Blockchain<Transaction>, node_state: &mut NodeState,
    stakes: &mut Blockchain<Transaction>,
) {
    match event {
        SwarmEvent::Behaviour(BlockchainBehaviourEvent::Gossipsub(
                                  GossipsubEvent::Message {
                                      propagation_source: peer_id,
                                      message_id: _id,
                                      message,
                                  })
        ) => {
            if let Ok(message) = serde_json::from_slice::<BlockchainMessage>(&message.data) {
                dispatch_blockchain_event(
                    swarm, transactions,
                    peer_id, message,
                    node_state, stakes,
                );
            }
        }
        SwarmEvent::Behaviour(BlockchainBehaviourEvent::Mdns(event)) => {
            dispatch_mdns(swarm, event, node_state)
        }
        _ => {}
    }
}

pub fn dispatch_mdns(
    swarm: &mut Swarm<BlockchainBehaviour>,
    event: Event, node_state: &mut NodeState,
) {
    match event {
        Event::Discovered(list) => {
            for (peer, addr) in list {
                println!("found {peer} {addr}");
                swarm.behaviour_mut().gossipsub().add_explicit_peer(&peer);
            }
        }
        Event::Expired(list) => {
            for (peer, addr) in list {
                println!("expired {peer} {addr}");
         //       node_state.kick(peer);
                if !swarm.behaviour_mut().mdns().has_node(&peer) {
                    swarm.behaviour_mut().gossipsub().remove_explicit_peer(&peer);
                }
            }
        }
    }
}

fn dispatch_blockchain_event(
    swarm: &mut Swarm<BlockchainBehaviour>,
    transactions: &mut Blockchain<Transaction>,
    sending_peer: PeerId, message: BlockchainMessage,
    node_state: &mut NodeState, stakes: &mut Blockchain<Transaction>,
) {
    match message {
        BlockchainMessage::SubmitTransaction {
            transaction,
            transaction_fee
        } => {
            transactions.add_uncommitted(transaction);
            transactions.add_uncommitted(transaction_fee);
            if transactions.has_enough_uncommitted_data() {
                let bid = node_state.user_wallet()
                    .to_wallet()
                    .balance(transactions, stakes) * 75 / 100;
                node_state.update_bid(StakeBid::bid(bid, node_state.user_wallet().address()));
                communication::publish_message(
                    swarm,
                    BlockchainMessage::Bid(node_state.node_bid().unwrap()),
                );
            }
        }
        BlockchainMessage::SubmitBlock { block_dto } => {
            on_submit_block(swarm, transactions, node_state, stakes, block_dto);
        }
        BlockchainMessage::Vote { block_valid } => {
            on_vote_received(transactions, sending_peer, node_state, block_valid);
        }
        BlockchainMessage::Bid(stake_bid) => {
            on_stake_raised(swarm, transactions, sending_peer, node_state, stakes, stake_bid);
        }
        BlockchainMessage::Join(wallet) => {
            if node_state.voting_in_progress() {
                communication::publish_message(swarm, BlockchainMessage::JoinDenied)
            } else {
                node_state.add_peer_wallet(sending_peer, wallet);
                communication::publish_message(
                    swarm, BlockchainMessage::Sync {
                        transactions: BlockchainDto::from(transactions),
                        wallets: node_state.wallets().clone(),
                        stakes: BlockchainDto::from(stakes),
                    },
                )
            }
        }
        _ => {}
    }
}

fn on_submit_block(
    swarm: &mut Swarm<BlockchainBehaviour>,
    transactions: &mut Blockchain<Transaction>,
    node_state: &mut NodeState,
    stakes: &mut Blockchain<Transaction>,
    block_dto: BlockDto<Transaction>,
) {
    let block_candidate = BlockCandidate::from(block_dto);
    let transaction_validator = TransactionValidator::new(
        node_state.wallets(), &stakes, &transactions,
    );
    let block_valid = match transaction_validator.block_valid(&block_candidate) {
        Ok(_) => true,
        Err(error) => {
            println!("{}", error.message());
            false
        }
    };
    node_state.set_pending_block(block_candidate);
    let vote = BlockchainMessage::Vote {
        block_valid
    };
    communication::publish_message(swarm, vote);
}

fn on_stake_raised(
    swarm: &mut Swarm<BlockchainBehaviour>,
    transactions: &mut Blockchain<Transaction>,
    sending_peer: PeerId, node_state: &mut NodeState,
    stakes: &mut Blockchain<Transaction>, stake_bid: StakeBid,
) {
    let wallet = node_state.wallets()
        .iter()
        .find(|wallet| wallet.address() == stake_bid.transaction().source_address());
    let wallet = match wallet {
        None => return,
        Some(wallet) => wallet.clone()
    };
    let balance = wallet.balance(transactions, stakes);
    if !balance < stake_bid.transaction().amount() {
        node_state.update_peers_bids(sending_peer, stake_bid);
        if node_state.all_bade() {
            let (winner, bid) = node_state.select_highest_bid();

            let stakes_block = match BlockCandidate::create_new(
                vec![bid.transaction().clone()], stakes.last_block(),
            ) {
                Ok(block) => block,
                Err(_) => panic!("No genesis block")
            };

            stakes.submit_new_block(stakes_block);
            if winner == *swarm.local_peer_id() {
                forge_block(swarm, transactions, node_state);
            }
            node_state.set_block_creator(winner.clone());
            node_state.reset_peer_bids();
        }
    } else {
        swarm.ban_peer_id(sending_peer);
        node_state.kick(sending_peer);
    }
}

fn on_vote_received(
    transactions: &mut Blockchain<Transaction>,
    sending_peer: PeerId, node_state: &mut NodeState, block_valid: bool,
) {
    let vote = Vote::new(sending_peer, block_valid);
    node_state.add_vote(vote);

    if node_state.all_voted() {
        let result = node_state.summarize_votes();
        if result.should_append_block() {
            let block_candidate = node_state.take_pending_block().unwrap();
            transactions.submit_new_block(block_candidate);
        }
    }
}

fn forge_block(
    swarm: &mut Swarm<BlockchainBehaviour>,
    transactions: &mut Blockchain<Transaction>,
    node_state: &mut NodeState,
) {
    match try_forge_block(transactions, node_state) {
        Ok(block_candidate) => {
            communication::publish_message(
                swarm,
                BlockchainMessage::SubmitBlock {
                    block_dto: BlockDto::from(block_candidate)
                },
            )
        }
        Err(error) => println!("{}", error.message())
    }
}

fn try_forge_block(
    transactions: &mut Blockchain<Transaction>, node_state: &mut NodeState,
) -> Result<BlockCandidate<Transaction>, Box<dyn BlockchainError>> {
    let data = transactions.uncommitted_data();
    let required_units = transactions.data_units_per_block();
    if data.len() < required_units as usize {
        return Err(Box::new(
            TransactionCountError::new(
                required_units, data.len() as u64,
            )));
    } else {
        let mut to_commit = Vec::new();
        to_commit.extend_from_slice(&data[..transactions.data_units_per_block() as usize]);
        let node_bid = node_state.node_bid().clone().unwrap();
        let wallet_node_address = node_state.user_wallet().address();
        to_commit.push(
            Transaction::stake_return(
                node_bid.stake(),
                wallet_node_address,
            )
        );
        to_commit.push(
            Transaction::forging_reward(wallet_node_address)
        );
        BlockCandidate::create_new(
            to_commit.clone(), transactions.last_block(),
        )
    }
}