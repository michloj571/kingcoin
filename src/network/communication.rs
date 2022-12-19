use serde::{Deserialize, Serialize};
use crate::blockchain::BlockchainData;
use crate::blockchain::core::Blockchain;
use crate::BlockHash;
use crate::network::BlockCandidate;

//todo implement message dispatching
#[derive(Serialize, Deserialize)]
pub enum Action<T> where T: BlockchainData {
    Validate(BlockCandidate<T>),
    VoteAdd(BlockHash),
    VoteReject(BlockHash),
}

pub fn dispatch<T>(action: Action<T>, handler: fn(Box<dyn BlockchainData>, &Blockchain<T>)) where T: BlockchainData {
    todo!()
}