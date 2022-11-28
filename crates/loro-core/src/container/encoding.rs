use serde::{ser::SerializeTuple, Deserialize, Serialize};
use serde_columnar::{compress, decompress, from_bytes, to_vec, CompressConfig};

use crate::{version::TotalOrderStamp, LoroValue};

use super::{
    list::ListContainer,
    map::{MapContainer, ValueSlot},
    pool::Pool,
    registry::ContainerInstance,
    text::TextContainer,
    Container, ContainerID,
};

pub fn split_u64_2_u32(a: u64) -> (u32, u32) {
    let high_byte = (a >> 32) as u32;
    let low_byte = a as u32;
    (high_byte, low_byte)
}

pub fn merge_2_u32_u64(a: u32, b: u32) -> u64 {
    let high_byte = (a as u64) << 32;
    let low_byte = b as u64;
    high_byte | low_byte
}

pub(crate) trait ContainerExport {
    type PoolItem: Serialize + for<'de> Deserialize<'de>;
    type BorrowPool<'a>: IntoIterator<Item = &'a Self::PoolItem>
    where
        Self: 'a;
    type Pool: IntoIterator<Item = Self::PoolItem>;
    type RangeItem: Serialize + for<'de> Deserialize<'de>;
    type Range: IntoIterator<Item = Self::RangeItem>;
    fn export_pool(&self) -> Self::BorrowPool<'_>;
    fn export_ranges(&self) -> Self::Range;
    fn import_pool(&mut self, pool: Self::Pool);
    fn import_ranges(&mut self, range: Self::Range);
}

impl ContainerExport for ListContainer {
    type PoolItem = LoroValue;
    type BorrowPool<'a> = Vec<&'a LoroValue>;
    type Pool = Vec<LoroValue>;
    type RangeItem = u32;
    type Range = Vec<u32>;

    fn export_pool(&self) -> Self::BorrowPool<'_> {
        self.raw_data.iter().collect()
    }

    fn export_ranges(&self) -> Self::Range {
        let ans = self
            .state
            .iter()
            .fold(Vec::with_capacity(self.state.len() * 2), |mut acc, p| {
                let s = p.get_sliced().0.start;
                let e = p.get_sliced().0.end;
                // print!("{}-{} ", s, e);
                acc.extend([s, e]);
                acc
            });
        // println!("\n");
        ans
        // .map(|slice_range| {
        //     merge_2_u32_u64(
        //         slice_range.get_sliced().0.start,
        //         slice_range.get_sliced().0.end,
        //     )
        // })
        // .collect()
    }

    fn import_pool(&mut self, pool: Self::Pool) {
        self.raw_data = Pool::new(pool);
    }

    fn import_ranges(&mut self, range: Self::Range) {
        let mut index = 0;
        for r in range.chunks(2) {
            let s = r[0];
            let e = r[1];
            self.state.insert(index, (s..e).into());
            index += (e - s) as usize;
        }
    }
}

impl ContainerExport for TextContainer {
    type PoolItem = u8;
    type BorrowPool<'a> = Vec<&'a u8>;
    type Pool = Vec<u8>;
    type RangeItem = u32;
    type Range = Vec<u32>;

    fn export_pool(&self) -> Self::BorrowPool<'_> {
        self.raw_str.data.iter().collect()
    }

    fn export_ranges(&self) -> Self::Range {
        let ans = self
            .state
            .iter()
            .fold(Vec::with_capacity(self.state.len() * 2), |mut acc, p| {
                let s = p.get_sliced().0.start;
                let e = p.get_sliced().0.end;
                // print!("{}-{} ", s, e);
                acc.extend([s, e]);
                acc
            });
        // println!("\n");
        ans
        // .map(|slice_range| {
        //     merge_2_u32_u64(
        //         slice_range.get_sliced().0.start,
        //         slice_range.get_sliced().0.end,
        //     )
        // })
        // .collect()
    }

    fn import_pool(&mut self, pool: Self::Pool) {
        self.raw_str.data = pool;
    }

    fn import_ranges(&mut self, range: Self::Range) {
        let mut index = 0;
        for r in range.chunks(2) {
            let s = r[0];
            let e = r[1];
            self.state.insert(index, (s..e).into());
            index += (e - s) as usize;
        }
    }
}

impl ContainerExport for MapContainer {
    type PoolItem = LoroValue;
    type BorrowPool<'a> = Vec<&'a LoroValue>;
    type Pool = Vec<LoroValue>;

    type RangeItem = (String, u32, TotalOrderStamp);

    type Range = Vec<(String, u32, TotalOrderStamp)>;

    fn export_pool(&self) -> Self::BorrowPool<'_> {
        self.pool.iter().collect()
    }

    fn export_ranges(&self) -> Self::Range {
        self.state
            .iter()
            .map(|(k, v)| (k.to_string(), v.value, v.order))
            .collect::<Vec<_>>()
    }

    fn import_pool(&mut self, pool: Self::Pool) {
        self.pool = Pool::new(pool);
    }

    fn import_ranges(&mut self, range: Self::Range) {
        for (k, v, o) in range.into_iter() {
            self.state
                .insert(k.into(), ValueSlot { value: v, order: o });
        }
    }
}

impl Serialize for ListContainer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut tuple = serializer.serialize_tuple(3)?;
        tuple.serialize_element(&self.id())?;
        tuple.serialize_element(&self.export_pool())?;
        tuple.serialize_element(&self.export_ranges())?;
        tuple.end()
    }
}

impl<'de> Deserialize<'de> for ListContainer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let (id, pool, range) = Deserialize::deserialize(deserializer)?;
        let mut container = Self::new(id);
        container.import_pool(pool);
        container.import_ranges(range);
        Ok(container)
    }
}

impl Serialize for TextContainer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut tuple = serializer.serialize_tuple(3)?;
        // let pool = compress(
        //     &self.export_pool().into_iter().copied().collect::<Vec<_>>(),
        //     &Default::default(),
        // )
        // .unwrap();
        tuple.serialize_element(&self.id())?;
        // tuple.serialize_element(&pool)?;
        tuple.serialize_element(&self.export_pool())?;
        tuple.serialize_element(&self.export_ranges())?;
        tuple.end()
    }
}

impl<'de> Deserialize<'de> for TextContainer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let (id, pool, range): (_, Vec<u8>, _) = Deserialize::deserialize(deserializer)?;
        let mut container = Self::new(id);
        // println!("pool: {:?}", &pool.len());
        // container.import_pool(decompress(&pool).unwrap());
        container.import_pool(pool);
        container.import_ranges(range);
        Ok(container)
    }
}

impl Serialize for MapContainer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut tuple = serializer.serialize_tuple(3)?;
        // let pool = compress(
        //     &self.export_pool().into_iter().copied().collect::<Vec<_>>(),
        //     &Default::default(),
        // )
        // .unwrap();
        tuple.serialize_element(&self.id())?;
        // tuple.serialize_element(&pool)?;
        tuple.serialize_element(&self.export_pool())?;
        tuple.serialize_element(&self.export_ranges())?;
        tuple.end()
    }
}

impl<'de> Deserialize<'de> for MapContainer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let (id, pool, range): (_, _, _) = Deserialize::deserialize(deserializer)?;
        let mut container = Self::new(id);
        // println!("pool: {:?}", &pool.len());
        // container.import_pool(decompress(&pool).unwrap());
        container.import_pool(pool);
        container.import_ranges(range);
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
