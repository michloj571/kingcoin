use std::error::Error;
use io::{BufReader};

use libp2p::{futures::StreamExt, Swarm};
use tokio::io::{self, AsyncBufReadExt};

use kingcoin::{
    blockchain::{core::Blockchain, Transaction, Wallet},
    network::{self, NodeState, communication::dispatch}
};
use kingcoin::network::BlockchainBehaviour;


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut swarm = network::configure_swarm();
    let (
        mut transactions,
        mut wallets,
        mut stakes
    ) = initialize_node(&mut swarm);

    let mut node_state = NodeState::init(swarm.local_peer_id().clone(),);
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
) -> (Blockchain<Transaction>, Blockchain<Wallet>, Blockchain<Transaction>) {
    let mut stakes = Blockchain::<Transaction>::transaction_chain(
        vec![],
    );
    let mut wallets = Blockchain::<Wallet>::wallet_chain();
    let mut transactions = Blockchain::<Transaction>::transaction_chain(
        vec![]
    );

    (transactions, wallets, stakes)
}

fn dispatch_command(command: Option<String>) -> bool {
todo!()
}