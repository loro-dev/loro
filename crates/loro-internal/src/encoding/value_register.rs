use fxhash::FxHashMap;

#[derive(Debug)]
pub struct ValueRegister<T> {
    map_value_to_index: FxHashMap<T, usize>,
    vec: Vec<T>,
}

impl<T: std::hash::Hash + Clone + PartialEq + Eq> Default for ValueRegister<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: std::hash::Hash + Clone + PartialEq + Eq> ValueRegister<T> {
    pub fn new() -> Self {
        Self {
            map_value_to_index: FxHashMap::default(),
            vec: Vec::new(),
        }
    }

    pub fn from_existing(vec: Vec<T>) -> Self {
        let mut map = FxHashMap::with_capacity_and_hasher(vec.len(), Default::default());
        for (i, value) in vec.iter().enumerate() {
            map.insert(value.clone(), i);
        }

        Self {
            map_value_to_index: map,
            vec,
        }
    }

    /// Return the index of the given value. If it does not exist,
    /// insert it and return the new index.
    pub fn register(&mut self, key: &T) -> usize {
        if let Some(index) = self.map_value_to_index.get(key) {
            *index
        } else {
            let idx = self.vec.len();
            self.vec.push(key.clone());
            self.map_value_to_index.insert(key.clone(), idx);
            idx
        }
    }

    pub fn get(&self, key: &T) -> Option<usize> {
        self.map_value_to_index.get(key).copied()
    }

    pub fn contains(&self, key: &T) -> bool {
        self.map_value_to_index.contains_key(key)
    }

    pub fn unwrap_vec(self) -> Vec<T> {
        self.vec
    }

    pub fn get_value(&self, index: usize) -> Option<&T> {
        self.vec.get(index)
    }

    pub fn vec(&self) -> &Vec<T> {
        &self.vec
    }
}
