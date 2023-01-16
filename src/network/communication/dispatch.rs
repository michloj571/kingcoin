use libp2p::{PeerId, Swarm};
use libp2p::gossipsub::GossipsubEvent;
use libp2p::mdns::Event;
use libp2p::swarm::SwarmEvent;

use crate::blockchain::{BlockchainData, StakeBid, Transaction, TransactionValidator, Wallet};
use crate::blockchain::core::{BlockCandidate, Blockchain, BlockchainError, TransactionCountError, Validate};
use crate::network::{BlockchainBehaviour, BlockchainBehaviourEvent, communication::{self, BlockDto, Vote}, NodeState};

use super::BlockchainMessage;

pub fn dispatch_network_event<H>(
    event: SwarmEvent<BlockchainBehaviourEvent, H>, swarm: &mut Swarm<BlockchainBehaviour>,
    transactions: &mut Blockchain<Transaction>, wallets: &mut Blockchain<Wallet>,
    node_state: &mut NodeState, stakes: &mut Blockchain<Transaction>,
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
                    swarm, transactions, wallets,
                    peer_id, message, node_state, stakes,
                );
            }
        }
        SwarmEvent::Behaviour(BlockchainBehaviourEvent::Mdns(event)) => {
            dispatch_mdns(swarm, event)
        }
        _ => {}
    }
}

fn dispatch_mdns(swarm: &mut Swarm<BlockchainBehaviour>, event: Event) {
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
    wallets: &mut Blockchain<Wallet>, sending_peer: PeerId,
    message: BlockchainMessage, node_state: &mut NodeState,
    stakes: &mut Blockchain<Transaction>,
) {
    match message {
        BlockchainMessage::SubmitTransaction(transaction) => {
            transactions.add_uncommitted(transaction);
            if transactions.has_enough_uncommitted_data() && node_state.should_create_block() {

            }
        }
        BlockchainMessage::SubmitBlock { block_dto } => on_submit_block(
            swarm, transactions, wallets, node_state, stakes, block_dto
        ),
        BlockchainMessage::Vote { block_valid } => on_vote_received(
            swarm, transactions, sending_peer, node_state, block_valid,
        ),
        BlockchainMessage::Bid(stake_bid) => on_stake_raised(
            swarm, transactions, sending_peer, node_state, stakes, stake_bid,
        ),
        BlockchainMessage::Sync { .. } => { todo!() }
    }
}

fn on_submit_block(
    swarm: &mut Swarm<BlockchainBehaviour>,
    transactions: &mut Blockchain<Transaction>,
    wallets: &mut Blockchain<Wallet>,
    node_state: &mut NodeState,
    stakes: &mut Blockchain<Transaction>,
    block_dto: BlockDto<Transaction>
) {
    let block_candidate = BlockCandidate::from(block_dto);
    let transaction_validator = TransactionValidator::new(
        &wallets, &stakes, &transactions,
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
    node_state.update_peers_bids(sending_peer, stake_bid);
    if node_state.all_bade(swarm.connected_peers().count()) {
        let (winner, bid) = node_state.select_highest_bid();

        let stakes_block = match BlockCandidate::create_new(
            vec![bid.transaction().clone()], stakes.last_block(),
        ) {
            Ok(block) => block,
            Err(_) => panic!("No genesis block")
        };

        stakes.submit_new_block(stakes_block);
        node_state.set_block_creator(winner.clone());
        if node_state.should_create_block() {
            match try_forge_block(transactions) {
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
        node_state.reset_peer_bids();
    }
}

fn on_vote_received(
    swarm: &mut Swarm<BlockchainBehaviour>, transactions: &mut Blockchain<Transaction>,
    sending_peer: PeerId, node_state: &mut NodeState, block_valid: bool,
) {
    let vote = Vote::new(sending_peer, block_valid);
    node_state.add_vote(vote);

    if node_state.all_voted(swarm.connected_peers().count()) {
        let result = node_state.summarize_votes();
        if result.should_append_block() {
            let block_candidate = node_state.take_pending_block().unwrap();
            transactions.submit_new_block(block_candidate);
        } else {
            node_state.mark_creator_bad().unwrap();
        }
    }
}

fn try_forge_block<T>(
    blockchain: &mut Blockchain<T>
) -> Result<BlockCandidate<T>, Box<dyn BlockchainError>> where T: BlockchainData {
    let data = blockchain.uncommitted_data();
    let required_units = blockchain.data_units_per_block();
    if data.len() < required_units as usize {
        return Err(Box::new(
            TransactionCountError::new(
                required_units, data.len() as u64,
            )));
    } else {
        let to_commit = &data[..blockchain.data_units_per_block() as usize].to_vec();
        BlockCandidate::create_new(
            to_commit.clone(), blockchain.last_block(),
        )
    }
}

pub fn submit_transaction(
    blockchain: &mut Blockchain<Transaction>, transaction: Transaction,
) -> BlockchainMessage {
    blockchain.add_uncommitted(transaction.clone());
    BlockchainMessage::SubmitTransaction(transaction)
}