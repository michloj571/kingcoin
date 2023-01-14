use std::collections::{HashMap, HashSet};
use std::mem;
use std::time::Duration;

use lazy_static::lazy_static;
use libp2p::{core::upgrade, gossipsub, identity::Keypair, mdns::{Event, tokio::Behaviour as TokioBehaviour}, mdns, mplex, noise, PeerId, Swarm, swarm::NetworkBehaviour, tcp::{Config, tokio::Transport as TokioTransport}, Transport};
use libp2p::gossipsub::{Gossipsub, GossipsubEvent, IdentTopic, MessageAuthenticity, ValidationMode};

use crate::blockchain::{StakeBid, Transaction};
use crate::blockchain::core::{BlockCandidate, BlockchainError};
use crate::network::communication::{Vote, VotingResult};

pub mod communication;

lazy_static! {
    pub static ref NETWORK_TOPIC: IdentTopic = IdentTopic::new("KINGCOIN");
}

pub struct NodeState {
    node_id: PeerId,
    node_bid: StakeBid,
    peers_bids: HashMap<PeerId, StakeBid>,
    block_creator: Option<PeerId>,
    bad_peers: HashSet<PeerId>,
    votes: HashSet<Vote>,
    pending_block: Option<BlockCandidate<Transaction>>,
}


impl NodeState {
    pub fn init(node_id: PeerId, initial_bid: StakeBid) -> NodeState {
        NodeState {
            node_id,
            node_bid: initial_bid,
            peers_bids: HashMap::new(),
            block_creator: None,
            bad_peers: HashSet::new(),
            votes: HashSet::new(),
            pending_block: None,
        }
    }

    pub fn node_id(&self) -> PeerId {
        self.node_id
    }

    pub fn node_bid(&self) -> &StakeBid {
        &self.node_bid
    }

    pub fn peers_bids(&self) -> &HashMap<PeerId, StakeBid> {
        &self.peers_bids
    }

    pub fn bad_peers(&self) -> &HashSet<PeerId> {
        &self.bad_peers
    }

    pub fn set_block_creator(&mut self, peer_id: PeerId) {
        self.block_creator = Some(peer_id);
    }

    pub fn set_pending_block(&mut self, pending_block: BlockCandidate<Transaction>) {
        self.pending_block = Some(pending_block);
    }

    pub fn update_peers_bids(&mut self, peer_id: PeerId, bid: StakeBid) {
        self.peers_bids.insert(peer_id, bid);
    }

    pub fn update_bid(&mut self, bid: StakeBid) {
        self.node_bid = bid;
    }

    pub fn all_bade(&self, peer_count: usize) -> bool {
        self.peers_bids.len() == peer_count
    }

    pub fn mark_creator_bad(&mut self) -> Result<(), ()> {
        match self.block_creator {
            None => Err(()),
            Some(creator) => {
                self.bad_peers.insert(creator);
                Ok(())
            }
        }
    }

    pub fn add_vote(&mut self, vote: Vote) {
        self.votes.insert(vote);
    }

    pub fn all_voted(&self, peer_count: usize) -> bool {
        self.votes.len() == peer_count
    }

    pub fn take_pending_block(&mut self) -> Option<BlockCandidate<Transaction>> {
        mem::take(&mut self.pending_block)
    }

    pub fn take_block_creator(&mut self) -> Option<PeerId> {
        mem::take(&mut self.block_creator)
    }

    pub fn summarize_votes(&self) -> VotingResult {
        let mut block_valid = 0;
        let mut block_invalid = 0;
        for vote in &self.votes {
            if vote.block_valid() {
                block_valid += 1;
            } else {
                block_invalid += 1;
            }
        }
        VotingResult::evaluate(block_valid, block_invalid)
    }

    pub fn select_highest_bid(&self) -> (&PeerId, &StakeBid) {
        let max_peer_bid = self.peers_bids
            .iter()
            .max_by(|first, second| {
                first.1.stake().cmp(&second.1.stake())
            }).unwrap();
        if max_peer_bid.1.stake() > self.node_bid.stake() {
            max_peer_bid
        } else {
            (&self.node_id, &self.node_bid)
        }
    }
    pub fn reset_peer_bids(&mut self) {
        self.peers_bids.clear();
    }
}

pub fn configure_swarm() -> Swarm<BlockchainBehaviour> {
    let key = Keypair::generate_ed25519();
    let local_id = PeerId::from(key.public());

    let gossipsub_config = gossipsub::GossipsubConfigBuilder::default()
        .heartbeat_interval(Duration::from_secs(10))
        .validation_mode(ValidationMode::Strict)
        //    .message_id_fn(message_id_fn)
        .build()
        .expect("Valid config");

    let transport = TokioTransport::new(Config::default().nodelay(true))
        .upgrade(upgrade::Version::V1)
        .authenticate(
            noise::NoiseAuthenticated::xx(&key)
                .expect("Signing libp2p-noise static DH keypair failed."),
        ).multiplex(mplex::MplexConfig::new())
        .boxed();
    let gossipsub = Gossipsub::new(MessageAuthenticity::Signed(key), gossipsub_config)
        .expect("Correct configuration");

    let mut behaviour = BlockchainBehaviour {
        gossipsub,
        mdns: TokioBehaviour::new(mdns::Config {
            ttl: Duration::MAX,
            query_interval: Duration::from_secs(1),
            enable_ipv6: false,
        }).unwrap(),
    };
    behaviour.gossipsub.subscribe(&NETWORK_TOPIC).expect("subscribe");

    Swarm::with_tokio_executor(transport, behaviour, local_id)
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "BlockchainBehaviourEvent")]
pub struct BlockchainBehaviour {
    gossipsub: Gossipsub,
    mdns: TokioBehaviour,
}

impl BlockchainBehaviour {
    pub fn gossipsub(&mut self) -> &mut Gossipsub {
        &mut self.gossipsub
    }

    pub fn mdns(&mut self) -> &mut TokioBehaviour {
        &mut self.mdns
    }
}

pub enum BlockchainBehaviourEvent {
    Gossipsub(GossipsubEvent),
    Mdns(Event),
}


impl From<GossipsubEvent> for BlockchainBehaviourEvent {
    fn from(event: GossipsubEvent) -> Self {
        BlockchainBehaviourEvent::Gossipsub(event)
    }
}

impl From<Event> for BlockchainBehaviourEvent {
    fn from(event: Event) -> Self {
        BlockchainBehaviourEvent::Mdns(event)
    }
}
