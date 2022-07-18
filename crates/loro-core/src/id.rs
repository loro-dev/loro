pub type ClientID = u64;
pub type Counter = u32;

#[derive(PartialEq, Eq, Hash, Clone, Debug, Copy, PartialOrd, Ord)]
pub struct ID {
    pub client_id: u64,
    pub counter: u32,
}

pub const ROOT_ID: ID = ID {
    client_id: u64::MAX,
    counter: u32::MAX,
};

impl ID {
    pub fn new(client_id: u64, counter: u32) -> Self {
        ID { client_id, counter }
    }

    pub fn null() -> Self {
        ROOT_ID
    }

    pub fn is_null(&self) -> bool {
        self.client_id == u64::MAX
    }

    #[inline]
    pub(crate) fn is_connected_id(&self, other: &Self, self_len: u32) -> bool {
        self.client_id == other.client_id && self.counter + self_len == other.counter
    }

    pub fn inc(&self, inc: u32) -> Self {
        ID {
            client_id: self.client_id,
            counter: self.counter + inc,
        }
    }
}
