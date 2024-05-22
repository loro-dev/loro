use serde::{de::Visitor, ser::SerializeMap, Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub enum OwnedFutureValue {
    #[cfg(feature = "counter")]
    Counter,
    // The future value cannot depend on the arena for encoding.
    Unknown(Unknown),
    JsonUnknown(JsonUnknown),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unknown {
    pub kind: u8,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonUnknown {
    pub value_type: String,
    pub value: String,
}

impl Serialize for OwnedFutureValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        match self {
            #[cfg(feature = "counter")]
            Self::Counter => {
                map.serialize_entry("value_type", "counter")?;
                map.end()
            }
            Self::Unknown(unknown) => {
                map.serialize_entry("value_type", "unknown")?;
                map.serialize_entry("value", &serde_json::to_string(unknown).unwrap())?;
                map.end()
            }
            Self::JsonUnknown(json_unknown) => {
                map.serialize_entry("value_type", "json_unknown")?;
                map.serialize_entry("value", &serde_json::to_string(json_unknown).unwrap())?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for OwnedFutureValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct _Visitor;

        impl<'de> Visitor<'de> for _Visitor {
            type Value = OwnedFutureValue;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str(" OwnedFutureValue ")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut value_type: Option<&str> = None;
                let mut value: Option<String> = None;
                while let Some(key) = map.next_key::<&str>()? {
                    match key {
                        "value_type" => {
                            value_type = Some(map.next_value()?);
                        }
                        "value" => {
                            value = Some(map.next_value()?);
                        }
                        _ => {
                            return Err(serde::de::Error::unknown_field(
                                key,
                                &["value_type", "value"],
                            ));
                        }
                    }
                }
                let value_type =
                    value_type.ok_or_else(|| serde::de::Error::missing_field("value_type"))?;

                match value_type {
                    #[cfg(feature = "counter")]
                    "counter" => Ok(OwnedFutureValue::Counter),
                    "unknown" => {
                        let value =
                            value.ok_or_else(|| serde::de::Error::missing_field("value"))?;
                        let unknown =
                            serde_json::from_str(&value).map_err(serde::de::Error::custom)?;
                        Ok(OwnedFutureValue::Unknown(unknown))
                    }
                    _ => {
                        let value = value.unwrap_or_default();
                        Ok(OwnedFutureValue::JsonUnknown(JsonUnknown {
                            value_type: value_type.to_owned(),
                            value: value.to_owned(),
                        }))
                    }
                }
            }
        }
        deserializer.deserialize_map(_Visitor)
    }
}
