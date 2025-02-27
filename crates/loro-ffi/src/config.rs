use std::sync::{Arc, RwLock};

use loro::StyleConfig;

#[derive(Default)]
pub struct Configure(loro::Configure);

impl Configure {
    pub fn fork(&self) -> Arc<Self> {
        Arc::new(Self(self.0.fork()))
    }

    pub fn record_timestamp(&self) -> bool {
        self.0.record_timestamp()
    }

    pub fn set_record_timestamp(&self, record: bool) {
        self.0.set_record_timestamp(record);
    }

    pub fn merge_interval(&self) -> i64 {
        self.0.merge_interval()
    }

    pub fn set_merge_interval(&self, interval: i64) {
        self.0.set_merge_interval(interval);
    }

    pub fn text_style_config(&self) -> Arc<StyleConfigMap> {
        Arc::new(StyleConfigMap(self.0.text_style_config().clone()))
    }
}

#[derive(Default, Debug)]
pub struct StyleConfigMap(Arc<RwLock<loro::StyleConfigMap>>);

impl StyleConfigMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, key: &str, value: StyleConfig) {
        self.0.write().unwrap().insert(key.into(), value);
    }

    pub fn get(&self, key: &str) -> Option<StyleConfig> {
        let m = self.0.read().unwrap();
        m.get(&(key.into()))
    }

    pub fn default_rich_text_config() -> Self {
        Self(Arc::new(RwLock::new(
            loro::StyleConfigMap::default_rich_text_config(),
        )))
    }

    pub(crate) fn to_loro(&self) -> loro::StyleConfigMap {
        self.0.read().unwrap().clone()
    }
}

impl From<loro::Configure> for Configure {
    fn from(value: loro::Configure) -> Self {
        Self(value)
    }
}
