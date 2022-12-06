use std::ops::Deref;

use serde::{ser::SerializeTuple, Deserialize, Serialize};
use serde_columnar::{from_bytes, to_vec};

use crate::{
    container::{
        list::ListContainer,
        map::{MapContainer, ValueSlot},
        registry::ContainerInstance,
        text::TextContainer,
        Container, ContainerID,
    },
    version::TotalOrderStamp,
    LoroValue,
};

impl Serialize for ListContainer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut tuple = serializer.serialize_tuple(3)?;
        let range = self
            .state
            .iter()
            .flat_map(|v| [v.get_sliced().0.start, v.get_sliced().0.end])
            .collect::<Vec<_>>();
        tuple.serialize_element(&self.id())?;
        tuple.serialize_element(self.raw_data.deref())?;
        tuple.serialize_element(&range)?;
        tuple.end()
    }
}

impl<'de> Deserialize<'de> for ListContainer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let (id, pool, range): (ContainerID, Vec<LoroValue>, Vec<u32>) =
            Deserialize::deserialize(deserializer)?;
        let mut container = Self::new(id);
        container.raw_data = pool.into();
        let mut index = 0;
        for r in range.chunks(2) {
            let s = r[0];
            let e = r[1];
            container.state.insert(index, (s..e).into());
            index += (e - s) as usize;
        }
        Ok(container)
    }
}

impl Serialize for TextContainer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut tuple = serializer.serialize_tuple(3)?;
        let pool = &self.raw_str.data;
        let range = &self
            .state
            .iter()
            .flat_map(|v| [v.get_sliced().0.start, v.get_sliced().0.end])
            .collect::<Vec<_>>();
        tuple.serialize_element(&self.id())?;
        tuple.serialize_element(pool)?;
        tuple.serialize_element(range)?;
        tuple.end()
    }
}

impl<'de> Deserialize<'de> for TextContainer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let (id, pool, range): (ContainerID, Vec<u8>, Vec<u32>) =
            Deserialize::deserialize(deserializer)?;
        let mut container = Self::new(id);
        container.raw_str.data = pool;
        let mut index = 0;
        for r in range.chunks(2) {
            let s = r[0];
            let e = r[1];
            container.state.insert(index, (s..e).into());
            index += (e - s) as usize;
        }
        Ok(container)
    }
}

impl Serialize for MapContainer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut tuple = serializer.serialize_tuple(3)?;
        let pool = self.pool.deref();
        let range = &self
            .state
            .iter()
            .map(|(k, v)| (k.to_string(), v.value, v.order))
            .collect::<Vec<_>>();
        tuple.serialize_element(&self.id())?;
        tuple.serialize_element(pool)?;
        tuple.serialize_element(range)?;
        tuple.end()
    }
}

impl<'de> Deserialize<'de> for MapContainer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let (id, pool, range): (
            ContainerID,
            Vec<LoroValue>,
            Vec<(String, u32, TotalOrderStamp)>,
        ) = Deserialize::deserialize(deserializer)?;
        let mut container = Self::new(id);
        container.pool = pool.into();
        for (k, v, o) in range.into_iter() {
            container
                .state
                .insert(k.into(), ValueSlot { value: v, order: o });
        }
        Ok(container)
    }
}

impl ContainerInstance {
    pub(crate) fn export_state(&self) -> Vec<u8> {
        to_vec(self).unwrap()
    }

    pub(crate) fn import_state(state: Vec<u8>) -> Self {
        from_bytes(&state).unwrap()
    }
}
