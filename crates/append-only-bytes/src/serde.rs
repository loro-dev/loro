use crate::BytesSlice;
use serde::{
    de::{SeqAccess, Visitor},
    Deserialize, Serialize,
};

impl Serialize for BytesSlice {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bytes(self.bytes())
    }
}

impl<'de> Deserialize<'de> for BytesSlice {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct BytesVisitor;
        impl<'de> Visitor<'de> for BytesVisitor {
            type Value = BytesSlice;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("BytesSliceVisitor deserialize failed")
            }

            fn visit_borrowed_bytes<E>(self, v: &'de [u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(BytesSlice::from_bytes(v))
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(BytesSlice::from_bytes(v))
            }

            fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let bytes: Vec<u8> = serde::de::Deserialize::deserialize(
                    serde::de::value::SeqAccessDeserializer::new(seq),
                )?;
                Ok(BytesSlice::from_bytes(&bytes))
            }
        }

        if deserializer.is_human_readable() {
            deserializer.deserialize_seq(BytesVisitor)
        } else {
            deserializer.deserialize_bytes(BytesVisitor)
        }
    }
}

#[cfg(test)]
mod test_serde {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test() {
        let mut a = HashMap::new();
        a.insert(1, BytesSlice::from_bytes(&[1, 2, 3]));
        a.insert(1, BytesSlice::from_bytes(&[3, 2, 3]));
        // test serde
        let s = serde_json::to_string(&a).unwrap();
        let b: HashMap<i32, BytesSlice> = serde_json::from_str(&s).unwrap();
        assert_eq!(a, b);
        // binary format
        let s = postcard::to_allocvec(&a).unwrap();
        let b: HashMap<i32, BytesSlice> = postcard::from_bytes(&s).unwrap();
        assert_eq!(a, b);
    }
}
