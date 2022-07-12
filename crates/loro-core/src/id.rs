pub type ClientID = u64;

#[derive(PartialEq, Eq, Hash, Clone, Debug, Copy, PartialOrd, Ord)]
pub struct ID {
    client_id: u64,
    counter: u32,
}
