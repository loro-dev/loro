pub mod container {
    use loro_common::ContainerID;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(container: &ContainerID, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_str(container.to_string().as_str())
    }

    pub fn deserialize<'de, D>(d: D) -> Result<ContainerID, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(d)?;
        ContainerID::try_from(s.as_str())
            .map_err(|_| serde::de::Error::custom("invalid container id"))
    }
}
