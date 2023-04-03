use loro_internal::{
    configure::Configure,
    container::ContainerIdRaw,
    event::{ObserverHandler, SubscriptionID},
    LoroCore, Transact, TransactionWrap,
};

pub use loro_internal::{
    container::ContainerIdx, event, id::ClientID, EncodeMode, List, LoroError, LoroValue, Map,
    Text, VersionVector,
};

#[repr(transparent)]
#[derive(Default)]
pub struct Loro(LoroCore);

impl Loro {
    #[inline(always)]
    pub fn new(cfg: Configure, client_id: Option<ClientID>) -> Self {
        Self(LoroCore::new(cfg, client_id))
    }

    #[inline(always)]
    pub fn client_id(&self) -> ClientID {
        self.0.client_id()
    }

    #[inline(always)]
    pub fn vv_cloned(&self) -> VersionVector {
        self.0.vv_cloned()
    }

    #[inline(always)]
    pub fn get_list<I: Into<ContainerIdRaw>>(&mut self, id_or_name: I) -> List {
        self.0.get_list(id_or_name)
    }

    #[inline(always)]
    pub fn get_map<I: Into<ContainerIdRaw>>(&mut self, id_or_name: I) -> Map {
        self.0.get_map(id_or_name)
    }

    #[inline(always)]
    pub fn get_text<I: Into<ContainerIdRaw>>(&mut self, id_or_name: I) -> Text {
        self.0.get_text(id_or_name)
    }

    #[inline(always)]
    pub fn encode_all(&self) -> Vec<u8> {
        self.0.encode_all()
    }

    #[inline(always)]
    pub fn encode_from(&self, from: VersionVector) -> Vec<u8> {
        self.0.encode_from(from)
    }

    #[inline(always)]
    pub fn encode_with_cfg(&self, mode: EncodeMode) -> Vec<u8> {
        self.0.encode_with_cfg(mode)
    }

    #[inline(always)]
    pub fn decode(&mut self, input: &[u8]) -> Result<(), LoroError> {
        self.0.decode(input)
    }

    #[inline(always)]
    pub fn to_json(&self) -> LoroValue {
        self.0.to_json()
    }

    #[inline(always)]
    pub fn subscribe_deep(&mut self, handler: ObserverHandler) -> SubscriptionID {
        self.0.subscribe_deep(handler)
    }

    #[inline(always)]
    pub fn unsubscribe_deep(&mut self, subscription: SubscriptionID) {
        self.0.unsubscribe_deep(subscription)
    }

    #[inline(always)]
    pub fn subscribe_once(&mut self, handler: ObserverHandler) -> SubscriptionID {
        self.0.subscribe_once(handler)
    }

    /// Execute with transaction
    #[inline(always)]
    pub fn txn(&mut self, f: impl FnOnce(TransactionWrap)) {
        f(self.transact())
    }
}

impl Transact for Loro {
    #[inline(always)]
    fn transact<'s: 'a, 'a>(&'s self) -> loro_internal::TransactionWrap<'a> {
        self.0.transact()
    }

    #[inline(always)]
    fn transact_with<'s: 'a, 'a>(
        &'s self,
        origin: Option<loro_internal::Origin>,
    ) -> TransactionWrap<'a> {
        self.0.transact_with(origin)
    }
}
