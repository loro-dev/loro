pub type PeerID = u32;
pub type Counter = i32;
pub type Lamport = u32;
pub struct ID {
    pub peer: PeerID,
    pub counter: Counter,
}
