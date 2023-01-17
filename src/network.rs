use std::collections::{HashMap, HashSet};
use std::mem;
use std::time::Duration;

use lazy_static::lazy_static;
use libp2p::{core::upgrade, gossipsub, identity::Keypair, mdns::{Event, tokio::Behaviour as TokioBehaviour}, mdns, mplex, noise, PeerId, Swarm, swarm::NetworkBehaviour, tcp::{Config, tokio::Transport as TokioTransport}, Transport};
use libp2p::gossipsub::{Gossipsub, GossipsubEvent, IdentTopic, MessageAuthenticity, ValidationMode};

use crate::blockchain::{HotWallet, MINTING_WALLET_ADDRESS, REWARD_WALLET_ADDRESS, STAKE_WALLET_ADDRESS, StakeBid, Transaction, Wallet};
use crate::blockchain::core::BlockCandidate;
use crate::network::communication::{Vote, VotingResult};

pub mod communication;

lazy_static! {
    pub static ref NETWORK_TOPIC: IdentTopic = IdentTopic::new("KINGCOIN");
}

pub struct NodeState {
    node_id: PeerId,
    user_wallet: HotWallet,
    node_bid: Option<StakeBid>,
    peers_bids: HashMap<PeerId, StakeBid>,
    voting: bool,
    wallets: HashSet<Wallet>,
    peers_wallets: HashMap<PeerId, Wallet>,
    block_creator: Option<PeerId>,
    votes: HashSet<Vote>,
    pending_block: Option<BlockCandidate<Transaction>>,
}


impl NodeState {
    pub fn init(
        node_id: PeerId, user_wallet: HotWallet,
    ) -> NodeState {
        let mut wallets = HashSet::new();
        wallets.insert(user_wallet.to_wallet());
        NodeState {
            node_id,
            user_wallet,
            node_bid: None,
            peers_bids: HashMap::new(),
            voting: false,
            wallets,
            peers_wallets: HashMap::new(),
            block_creator: None,
            votes: HashSet::new(),
            pending_block: None,
        }
    }

    fn default_wallets() -> HashSet<Wallet> {
        let mut wallets = HashSet::new();
        wallets.insert(
            Wallet::new(
                MINTING_WALLET_ADDRESS, None,
            )
        );
        wallets.insert(
            Wallet::new(
                *REWARD_WALLET_ADDRESS, None,
            )
        );
        wallets.insert(
            Wallet::new(
                *STAKE_WALLET_ADDRESS, None,
            )
        );
        wallets
    }

    pub fn node_id(&self) -> PeerId {
        self.node_id
    }

    pub fn wallets(&self) -> &HashSet<Wallet> {
        &self.wallets
    }

    pub fn add_wallets(&mut self, wallets: HashSet<Wallet>) {
        self.wallets.extend(wallets);
    }

    pub fn user_wallet(&self) -> &HotWallet {
        &self.user_wallet
    }

    pub fn node_bid(&self) -> Option<StakeBid> {
        self.node_bid.clone()
    }

    pub fn peers_bids(&self) -> &HashMap<PeerId, StakeBid> {
        &self.peers_bids
    }

    pub fn should_create_block(&self) -> bool {
        match self.block_creator {
            None => false,
            Some(peer_id) => self.node_id == peer_id
        }
    }

    pub fn add_peer_wallet(&mut self, peer_id: PeerId, wallet: Wallet) -> Option<Wallet> {
        self.wallets.insert(wallet.clone());
        self.peers_wallets.insert(peer_id, wallet)
    }

    pub fn voting_in_progress(&self) -> bool {
        self.voting
    }

    pub fn set_block_creator(&mut self, id: PeerId) {
        self.block_creator = Some(id);
    }

    pub fn set_pending_block(&mut self, pending_block: BlockCandidate<Transaction>) {
        self.pending_block = Some(pending_block);
    }

    pub fn update_peers_bids(&mut self, peer_id: PeerId, bid: StakeBid) {
        self.peers_bids.insert(peer_id, bid);
    }

    pub fn update_bid(&mut self, bid: StakeBid) {
        self.node_bid = Some(bid);
    }

    pub fn all_bade(&self) -> bool {
        self.peers_bids.len() == self.wallets.len() - 1
    }

    pub fn add_vote(&mut self, vote: Vote) {
        self.voting = true;
        self.votes.insert(vote);
    }

    pub fn all_voted(&self) -> bool {
        self.votes.len() == self.wallets.len() - 1
    }

    pub fn take_pending_block(&mut self) -> Option<BlockCandidate<Transaction>> {
        mem::take(&mut self.pending_block)
    }

    pub fn take_block_creator(&mut self) -> Option<PeerId> {
        mem::take(&mut self.block_creator)
    }

    pub fn summarize_votes(&mut self) -> VotingResult {
        let mut block_valid = 0;
        let mut block_invalid = 0;
        for vote in &self.votes {
            if vote.block_valid() {
                block_valid += 1;
            } else {
                block_invalid += 1;
            }
        }
        self.votes.clear();
        self.voting = false;
        VotingResult::evaluate(block_valid, block_invalid)
    }

    pub fn select_highest_bid(&self) -> (PeerId, StakeBid) {
        let (peer_id, peer_bid) = self.peers_bids
            .iter()
            .max_by(|first, second| {
                first.1.stake().cmp(&second.1.stake())
            }).unwrap();

        self.node_bid.clone().map_or((peer_id.clone(), peer_bid.clone()), |bid| {
            if peer_bid.stake() > bid.stake() {
                (peer_id.clone(), peer_bid.clone())
            } else {
                (self.node_id.clone(), bid.clone())
            }
        })
    }

    pub fn reset_peer_bids(&mut self) {
        self.peers_bids.clear();
    }

    pub fn kick(&mut self, peer: PeerId) {
        let wallet = self.peers_wallets.remove(&peer).unwrap();
        self.wallets.remove(&wallet);
        self.peers_bids.remove(&peer);
    }
    pub fn set_wallets(&mut self, wallets: HashSet<Wallet>) {
        self.wallets = wallets;
    }
}

pub fn configure_swarm() -> Swarm<BlockchainBehaviour> {
    let key = Keypair::generate_ed25519();
    let local_id = PeerId::from(key.public());

    let gossipsub_config = gossipsub::GossipsubConfigBuilder::default()
        .heartbeat_interval(Duration::from_secs(10))
        .validation_mode(ValidationMode::Strict)
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
