use crate::id::ID;

use std::collections::HashMap;

pub struct AntiGraph(HashMap<ID, Vec<ID>>);

impl AntiGraph {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn init(&mut self, id: &ID) {
        if !self.0.contains_key(&id) {
            self.0.insert(id.clone(), vec![]);
        }
    }

    pub fn add(&mut self, from_id: &ID, to_id: &ID) {
        self.0.entry(*from_id).or_default().push(*to_id);
    }

    pub fn get<'a>(&'a self, from_id: &ID) -> &'a Vec<ID> {
        self.0.get(from_id).unwrap()
    }
}

pub struct DeepOrInd(HashMap<ID, usize>);

impl DeepOrInd {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn init(&mut self, id: &ID) {
        self.0.insert(*id, 0);
    }

    pub fn inc(&mut self, id: &ID) {
        if let Some(x) = self.0.get_mut(id) {
            *x += 1;
        } else {
            self.0.insert(*id, 1);
        }
    }

    pub fn dec(&mut self, id: &ID) -> usize {
        let x = self.0.get_mut(id).unwrap();
        *x -= 1;
        *x
    }

    pub fn get(&self, id: &ID) -> usize {
        if let Some(x) = self.0.get(id) {
            *x
        } else {
            0
        }
    }

    pub fn set(&mut self, id: &ID, val: usize) {
        let x = self.0.get_mut(id).unwrap();
        *x = val;
    }
}

pub struct Father(HashMap<ID, Vec<ID>>);

impl Father {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn init(&mut self, id: &ID, scale: &usize) {
        self.0.insert(
            *id,
            vec![
                ID {
                    peer: 0,
                    counter: -1
                };
                scale + 1
            ],
        );
    }

    pub fn get(&self, id: &ID, layer: usize) -> ID {
        self.0.get(id).map_or(
            ID {
                peer: 0,
                counter: 0,
            },
            |x| x[layer],
        )
    }

    pub fn set(&mut self, id: &ID, layer: usize, value: ID) {
        self.0.get_mut(id).unwrap()[layer] = value;
    }
}
