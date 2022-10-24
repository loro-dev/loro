use fxhash::FxHashMap;
use loro_core::{
    id::{ClientID, Counter},
    version::VersionVector,
};
use rle::RleVecWithIndex;

use crate::raw_change::{ChangeData, ChangeHash};

pub type Mac = [u8; 32];

pub struct RawStore {
    changes: FxHashMap<ClientID, RleVecWithIndex<ChangeData>>,
    macs: Option<FxHashMap<ClientID, Mac>>,
}

impl RawStore {
    pub fn new() -> Self {
        Self {
            changes: FxHashMap::default(),
            macs: None,
        }
    }

    pub fn maced(&self) -> bool {
        self.macs.is_some()
    }

    pub fn encode_update(&self, _from: Option<&VersionVector>) -> Vec<u8> {
        unimplemented!()
    }

    pub fn verify(&mut self, _pub_key: &[u8; 32]) -> bool {
        if !self.maced() {
            return true;
        }

        if self.macs.as_ref().unwrap().len() < self.changes.len() {
            return false;
        }

        self.calc_hash();
        for (_client_id, _mac) in self.macs.as_ref().unwrap().iter() {
            todo!("pending");
        }

        true
    }

    pub fn get_final_hash(&self, client_id: ClientID) -> ChangeHash {
        let changes = self.changes.get(&client_id).unwrap();
        let last = changes.vec().last().unwrap();
        last.hash.unwrap()
    }

    fn calc_hash(&mut self) {
        for (_client_id, changes) in &mut self.changes.iter_mut() {
            let changes = changes.vec_mut();
            let mut start_index = 0;
            for i in (0..changes.len()).rev() {
                if changes[i].hash.is_some() {
                    start_index = i + 1;
                    break;
                }
            }
            for index in start_index..changes.len() {
                let (prev, cur) = changes.split_at_mut(index);
                cur[0].update_hash(if index == 0 {
                    None
                } else {
                    Some(prev.last().unwrap().hash.unwrap())
                });
            }
        }
    }

    pub fn version_vector(&self) -> VersionVector {
        let mut version_vector = VersionVector::new();
        for (client_id, changes) in &self.changes {
            version_vector.insert(*client_id, changes.len() as Counter);
        }

        version_vector
    }

    pub fn sign(&self, _pub_key: ()) {
        unimplemented!()
    }
}

impl Default for RawStore {
    fn default() -> Self {
        Self::new()
    }
}
