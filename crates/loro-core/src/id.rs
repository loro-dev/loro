use serde::Serialize;

pub type ClientID = u64;
pub type Counter = i32;

#[derive(PartialEq, Eq, Hash, Clone, Debug, Copy, PartialOrd, Ord, Serialize)]
pub struct ID {
    pub client_id: u64,
    pub counter: Counter,
}

pub const ROOT_ID: ID = ID {
    client_id: u64::MAX,
    counter: i32::MAX,
};

impl ID {
    pub fn new(client_id: u64, counter: Counter) -> Self {
        ID { client_id, counter }
    }

    pub fn null() -> Self {
        ROOT_ID
    }

    pub fn is_null(&self) -> bool {
        self.client_id == u64::MAX
    }

    #[inline]
    pub(crate) fn is_connected_id(&self, other: &Self, self_len: usize) -> bool {
        self.client_id == other.client_id && self.counter + self_len as Counter == other.counter
    }

    pub fn inc(&self, inc: i32) -> Self {
        ID {
            client_id: self.client_id,
            counter: self.counter + inc,
        }
    }
}
