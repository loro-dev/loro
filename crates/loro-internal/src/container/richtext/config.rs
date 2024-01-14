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
        self.map.insert(key, value);
    }

    pub fn get(&self, key: &InternalString) -> Option<&StyleConfig> {
        self.map.get(key)
    }

    pub fn get_style_flag(&self, key: &InternalString) -> Option<TextStyleInfoFlag> {
        self.map
            .get(key)
            .map(|x| TextStyleInfoFlag::new(!x.allow_overlap, x.expand))
    }

    pub fn get_style_flag_for_unmark(&self, key: &InternalString) -> Option<TextStyleInfoFlag> {
        self.map
            .get(key)
            .map(|x| TextStyleInfoFlag::new(!x.allow_overlap, x.expand.reverse()))
    }

    pub fn default_rich_text_config() -> Self {
        let mut map = Self {
            map: FxHashMap::default(),
        };

        map.map.insert(
            "bold".into(),
            StyleConfig {
                allow_overlap: false,
                expand: ExpandType::After,
            },
        );

        map.map.insert(
            "italic".into(),
            StyleConfig {
                allow_overlap: false,
                expand: ExpandType::After,
            },
        );

        map.map.insert(
            "underline".into(),
            StyleConfig {
                allow_overlap: false,
                expand: ExpandType::After,
            },
        );

        map.map.insert(
            "link".into(),
            StyleConfig {
                allow_overlap: false,
                expand: ExpandType::None,
            },
        );

        map.map.insert(
            "highlight".into(),
            StyleConfig {
                allow_overlap: false,
                expand: ExpandType::None,
            },
        );

        map.map.insert(
            "comment".into(),
            StyleConfig {
                allow_overlap: true,
                expand: ExpandType::None,
            },
        );

        map
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleConfig {
    pub allow_overlap: bool,
    pub expand: ExpandType,
}

impl StyleConfig {
    pub fn new() -> Self {
        Self {
            allow_overlap: false,
            expand: ExpandType::None,
        }
    }

    pub fn allow_overlap(mut self, allow_overlap: bool) -> Self {
        self.allow_overlap = allow_overlap;
        self
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
