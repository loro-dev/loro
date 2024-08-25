use fxhash::FxHashMap;
use loro_common::InternalString;

use super::{ExpandType, TextStyleInfoFlag};

#[derive(Debug, Default, Clone)]
pub struct StyleConfigMap {
    map: FxHashMap<InternalString, StyleConfig>,
}

impl StyleConfigMap {
    pub fn new() -> Self {
        Self {
            map: FxHashMap::default(),
        }
    }

    pub fn insert(&mut self, key: InternalString, value: StyleConfig) {
        if key.contains(':') {
            panic!("style key should not contain ':'");
        }

        self.map.insert(key, value);
    }

    pub fn get(&self, key: &InternalString) -> Option<&StyleConfig> {
        self.map.get(key)
    }

    pub fn get_style_flag(&self, key: &InternalString) -> Option<TextStyleInfoFlag> {
        self._get_style_flag(key, false)
    }

    pub fn get_style_flag_for_unmark(&self, key: &InternalString) -> Option<TextStyleInfoFlag> {
        self._get_style_flag(key, true)
    }

    fn _get_style_flag(&self, key: &InternalString, is_del: bool) -> Option<TextStyleInfoFlag> {
        let f = |x: &StyleConfig| {
            TextStyleInfoFlag::new(if is_del { x.expand.reverse() } else { x.expand })
        };
        if let Some(index) = key.find(':') {
            let key = key[..index].into();
            self.map.get(&key).map(f)
        } else {
            self.map.get(key).map(f)
        }
    }

    pub fn default_rich_text_config() -> Self {
        let mut map = Self {
            map: FxHashMap::default(),
        };

        map.map.insert(
            "bold".into(),
            StyleConfig {
                expand: ExpandType::After,
            },
        );

        map.map.insert(
            "italic".into(),
            StyleConfig {
                expand: ExpandType::After,
            },
        );

        map.map.insert(
            "underline".into(),
            StyleConfig {
                expand: ExpandType::After,
            },
        );

        map.map.insert(
            "link".into(),
            StyleConfig {
                expand: ExpandType::None,
            },
        );

        map.map.insert(
            "highlight".into(),
            StyleConfig {
                expand: ExpandType::None,
            },
        );

        map.map.insert(
            "comment".into(),
            StyleConfig {
                expand: ExpandType::None,
            },
        );
        
        map.map.insert(
            "code".into(),
            StyleConfig {
                expand: ExpandType::None,
            },
        );

        map
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleConfig {
    pub expand: ExpandType,
}

impl StyleConfig {
    pub fn new() -> Self {
        Self {
            expand: ExpandType::None,
        }
    }

    pub fn expand(mut self, expand: ExpandType) -> Self {
        self.expand = expand;
        self
    }
}

impl Default for StyleConfig {
    fn default() -> Self {
        Self::new()
    }
}
