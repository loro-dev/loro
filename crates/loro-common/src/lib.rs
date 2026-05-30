use std::{fmt::Display, io::Write};

use arbitrary::Arbitrary;
use enum_as_inner::EnumAsInner;

use nonmax::{NonMaxI32, NonMaxU32};
use serde::{Deserialize, Serialize};

mod error;
mod id;
mod internal_string;
mod logging;
mod macros;
mod span;
mod value;

pub use error::{LoroEncodeError, LoroError, LoroResult, LoroTreeError};
pub use internal_string::InternalString;
pub use logging::log::*;
#[doc(hidden)]
pub use rustc_hash::FxHashMap;
pub use span::*;
pub use value::{
    to_value, LoroBinaryValue, LoroListValue, LoroMapValue, LoroStringValue, LoroValue,
};

/// Unique id for each peer. It's a random u64 by default.
pub type PeerID = u64;
/// If it's the nth Op of a peer, the counter will be n.
pub type Counter = i32;
/// It's the [Lamport clock](https://en.wikipedia.org/wiki/Lamport_timestamp)
pub type Lamport = u32;

/// It's the unique ID of an Op represented by [PeerID] and [Counter].
#[derive(PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
pub struct ID {
    pub peer: PeerID,
    pub counter: Counter,
}

impl ID {
    pub fn to_bytes(&self) -> [u8; 12] {
        let mut bytes = [0; 12];
        bytes[..8].copy_from_slice(&self.peer.to_be_bytes());
        bytes[8..].copy_from_slice(&self.counter.to_be_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        if bytes.len() != 12 {
            panic!(
                "Invalid ID bytes. Expected 12 bytes but got {} bytes",
                bytes.len()
            );
        }

        Self {
            peer: u64::from_be_bytes(bytes[..8].try_into().unwrap()),
            counter: i32::from_be_bytes(bytes[8..].try_into().unwrap()),
        }
    }
}

/// It's the unique ID of an Op represented by [PeerID] and [Counter].
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct CompactId {
    pub peer: PeerID,
    pub counter: NonMaxI32,
}

/// Namespace prefix used to encode mergeable container IDs into Root container names.
///
/// The 🤝 ("handshake") sentinel mirrors Loro's existing 🦜 brand convention
/// (`crates/loro-common/src/value.rs::LORO_CONTAINER_ID_PREFIX`) and signals
/// "two peers agreeing on the same cid." The trailing `:` separates the brand
/// from the hex-encoded `(parent, key, container_type)` payload; hex characters
/// (`0-9a-f`) cannot collide with the prefix bytes, so no closing sentinel is
/// needed. User-created root container names are rejected by
/// `check_root_container_name` if they start with this prefix.
pub const MERGEABLE_NAMESPACE_PREFIX: &str = "🤝:";

fn write_len_prefixed_segment(out: &mut Vec<u8>, bytes: &[u8]) {
    leb128::write::unsigned(out, bytes.len() as u64).unwrap();
    out.extend_from_slice(bytes);
}

fn read_len_prefixed_segment(input: &mut &[u8]) -> Option<Vec<u8>> {
    let len = leb128::read::unsigned(input).ok()? as usize;
    if input.len() < len {
        return None;
    }
    let (segment, rest) = input.split_at(len);
    *input = rest;
    Some(segment.to_vec())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    if !s.len().is_multiple_of(2) {
        return None;
    }

    let mut out = Vec::with_capacity(s.len() / 2);
    for pair in s.as_bytes().chunks_exact(2) {
        out.push((value(pair[0])? << 4) | value(pair[1])?);
    }
    Some(out)
}

/// Return whether the given name is a valid root container name.
pub fn check_root_container_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(MERGEABLE_NAMESPACE_PREFIX)
        && name.char_indices().all(|(_, x)| x != '/' && x != '\0')
}

/// Build the discriminator string a parent map stores at a mergeable key.
///
/// The value written into the parent map's slot is `LoroValue::String("🤝:<kind>")`,
/// reusing [`MERGEABLE_NAMESPACE_PREFIX`] so the string is recognizable as a
/// mergeable sentinel (and so older clients that don't understand it at least
/// see a value in the reserved namespace rather than a plausible user string).
///
/// The active mergeable child at `(parent, key)` is whichever discriminator the
/// parent map's regular LWW resolves to; the corresponding deterministic cid is
/// [`ContainerID::new_mergeable(parent, key, kind)`]. See loro-dev/loro#759.
pub fn mergeable_discriminator_string(container_type: ContainerType) -> String {
    format!("{}{}", MERGEABLE_NAMESPACE_PREFIX, container_type)
}

/// Build the [`LoroValue`] a parent map stores at a mergeable key for `container_type`.
///
/// This is the value form of [`mergeable_discriminator_string`]; it is what
/// `get_mergeable_<kind>` writes via a `MapSet` op against the parent map.
pub fn mergeable_discriminator(container_type: ContainerType) -> LoroValue {
    LoroValue::String(mergeable_discriminator_string(container_type).into())
}

/// Parse a parent map slot value back into the [`ContainerType`] it discriminates,
/// or `None` if the value is not a recognized mergeable discriminator.
///
/// Inverse of [`mergeable_discriminator`]. Returns `None` for non-string values,
/// strings that don't carry the mergeable prefix, the bare prefix, and prefixed
/// strings whose suffix is not a known container-type name.
pub fn parse_mergeable_discriminator(value: &LoroValue) -> Option<ContainerType> {
    let LoroValue::String(s) = value else {
        return None;
    };
    let suffix = s.as_str().strip_prefix(MERGEABLE_NAMESPACE_PREFIX)?;
    // `ContainerType::try_from` accepts the canonical `Display` names ("Map",
    // "List", ...) plus the `Unknown(n)` form. A bare/empty suffix is rejected.
    ContainerType::try_from(suffix).ok()
}

impl CompactId {
    pub fn new(peer: PeerID, counter: Counter) -> Self {
        Self {
            peer,
            counter: NonMaxI32::new(counter).unwrap(),
        }
    }

    pub fn to_id(&self) -> ID {
        ID {
            peer: self.peer,
            counter: self.counter.get(),
        }
    }

    pub fn inc(&self, start: i32) -> CompactId {
        Self {
            peer: self.peer,
            counter: NonMaxI32::new(start + self.counter.get()).unwrap(),
        }
    }
}

impl TryFrom<ID> for CompactId {
    type Error = ID;

    fn try_from(id: ID) -> Result<Self, ID> {
        if id.counter == i32::MAX {
            return Err(id);
        }

        Ok(Self::new(id.peer, id.counter))
    }
}

/// It's the unique ID of an Op represented by [PeerID] and [Lamport] clock.
/// It's used to define the total order of Ops.
#[derive(PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize, PartialOrd, Ord)]
pub struct IdLp {
    pub lamport: Lamport,
    pub peer: PeerID,
}

impl IdLp {
    pub fn compact(self) -> CompactIdLp {
        CompactIdLp::new(self.peer, self.lamport)
    }
}

/// It's the unique ID of an Op represented by [PeerID] and [Counter].
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct CompactIdLp {
    pub peer: PeerID,
    pub lamport: NonMaxU32,
}

impl CompactIdLp {
    pub fn new(peer: PeerID, lamport: Lamport) -> Self {
        Self {
            peer,
            lamport: NonMaxU32::new(lamport).unwrap(),
        }
    }

    pub fn to_id(&self) -> IdLp {
        IdLp {
            peer: self.peer,
            lamport: self.lamport.get(),
        }
    }
}

impl TryFrom<IdLp> for CompactIdLp {
    type Error = IdLp;

    fn try_from(id: IdLp) -> Result<Self, IdLp> {
        if id.lamport == u32::MAX {
            return Err(id);
        }

        Ok(Self::new(id.peer, id.lamport))
    }
}

/// It's the unique ID of an Op represented by [PeerID], [Lamport] clock and [Counter].
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
pub struct IdFull {
    pub peer: PeerID,
    pub lamport: Lamport,
    pub counter: Counter,
}

/// [ContainerID] includes the Op's [ID] and the type. So it's impossible to have
/// the same [ContainerID] with conflict [ContainerType].
///
/// This structure is really cheap to clone.
///
/// String representation:
///
/// - Root Container: `/<name>:<type>`
/// - Normal Container: `<counter>@<client>:<type>`
///
/// Note: It will be encoded into binary format, so the order of its fields should not be changed.
#[derive(Hash, PartialEq, Eq, Clone, Serialize, Deserialize, EnumAsInner)]
pub enum ContainerID {
    /// Root container does not need an op to create. It can be created implicitly.
    Root {
        name: InternalString,
        container_type: ContainerType,
    },
    Normal {
        peer: PeerID,
        counter: Counter,
        container_type: ContainerType,
    },
}

impl ContainerID {
    pub fn encode<W: Write>(&self, writer: &mut W) -> Result<(), std::io::Error> {
        match self {
            Self::Root {
                name,
                container_type,
            } => {
                let first_byte = container_type.to_u8() | 0b10000000;
                writer.write_all(&[first_byte])?;
                leb128::write::unsigned(writer, name.len() as u64)?;
                writer.write_all(name.as_bytes())?;
            }
            Self::Normal {
                peer,
                counter,
                container_type,
            } => {
                let first_byte = container_type.to_u8();
                writer.write_all(&[first_byte])?;
                writer.write_all(&peer.to_le_bytes())?;
                writer.write_all(&counter.to_le_bytes())?;
            }
        }

        Ok(())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        // normal need 13 bytes
        let mut bytes = Vec::with_capacity(13);
        self.encode(&mut bytes).unwrap();
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self::try_from_bytes(bytes).unwrap()
    }

    pub fn try_from_bytes(bytes: &[u8]) -> LoroResult<Self> {
        if bytes.is_empty() {
            return Err(LoroError::DecodeError(
                "Decode container id failed".to_string().into_boxed_str(),
            ));
        }

        let first_byte = bytes[0];
        let container_type = ContainerType::try_from_u8(first_byte & 0b01111111)?;
        let is_root = (first_byte & 0b10000000) != 0;

        let mut reader = &bytes[1..];
        match is_root {
            true => {
                let name_len = leb128::read::unsigned(&mut reader).map_err(|_| {
                    LoroError::DecodeError(
                        "Decode container id failed".to_string().into_boxed_str(),
                    )
                })?;
                let name_len = usize::try_from(name_len).map_err(|_| {
                    LoroError::DecodeError(
                        "Decode container id failed".to_string().into_boxed_str(),
                    )
                })?;
                if reader.len() != name_len {
                    return Err(LoroError::DecodeError(
                        "Decode container id failed".to_string().into_boxed_str(),
                    ));
                }

                let name = std::str::from_utf8(&reader[..name_len]).map_err(|_| {
                    LoroError::DecodeError(
                        "Decode container id failed".to_string().into_boxed_str(),
                    )
                })?;
                Ok(Self::Root {
                    name: InternalString::from(name),
                    container_type,
                })
            }
            false => {
                if reader.len() != 12 {
                    return Err(LoroError::DecodeError(
                        "Decode container id failed".to_string().into_boxed_str(),
                    ));
                }

                let peer = PeerID::from_le_bytes(reader[..8].try_into().unwrap());
                let counter = i32::from_le_bytes(reader[8..12].try_into().unwrap());
                Ok(Self::Normal {
                    peer,
                    counter,
                    container_type,
                })
            }
        }
    }

    const LORO_CONTAINER_ID_PREFIX: &str = "🦜:";
    pub fn to_loro_value_string(&self) -> String {
        format!("{}{}", Self::LORO_CONTAINER_ID_PREFIX, self)
    }

    pub fn try_from_loro_value_string(s: &str) -> Option<Self> {
        if let Some(s) = s.strip_prefix(Self::LORO_CONTAINER_ID_PREFIX) {
            Self::try_from(s).ok()
        } else {
            None
        }
    }
}

impl std::fmt::Debug for ContainerID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Root {
                name,
                container_type,
            } => {
                write!(f, "Root(\"{name}\" {container_type:?})")
            }
            Self::Normal {
                peer,
                counter,
                container_type,
            } => {
                write!(f, "Normal({container_type:?} {counter}@{peer})")
            }
        }
    }
}

// TODO: add non_exhausted
// Note: It will be encoded into binary format, so the order of its fields should not be changed.
#[derive(Arbitrary, Debug, PartialEq, Eq, Hash, Clone, Copy, PartialOrd, Ord)]
pub enum ContainerType {
    Text,
    Map,
    List,
    MovableList,
    Tree,
    #[cfg(feature = "counter")]
    Counter,
    Unknown(u8),
}

impl ContainerType {
    #[cfg(feature = "counter")]
    pub const ALL_TYPES: [ContainerType; 6] = [
        ContainerType::Map,
        ContainerType::List,
        ContainerType::Text,
        ContainerType::Tree,
        ContainerType::MovableList,
        ContainerType::Counter,
    ];
    #[cfg(not(feature = "counter"))]
    pub const ALL_TYPES: [ContainerType; 5] = [
        ContainerType::Map,
        ContainerType::List,
        ContainerType::Text,
        ContainerType::Tree,
        ContainerType::MovableList,
    ];

    pub fn default_value(&self) -> LoroValue {
        match self {
            ContainerType::Map => LoroValue::Map(Default::default()),
            ContainerType::List => LoroValue::List(Default::default()),
            ContainerType::Text => LoroValue::String(Default::default()),
            ContainerType::Tree => LoroValue::List(Default::default()),
            ContainerType::MovableList => LoroValue::List(Default::default()),
            #[cfg(feature = "counter")]
            ContainerType::Counter => LoroValue::Double(0.),
            ContainerType::Unknown(_) => unreachable!(),
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            ContainerType::Map => 0,
            ContainerType::List => 1,
            ContainerType::Text => 2,
            ContainerType::Tree => 3,
            ContainerType::MovableList => 4,
            #[cfg(feature = "counter")]
            ContainerType::Counter => 5,
            ContainerType::Unknown(k) => k,
        }
    }

    pub fn try_from_u8(v: u8) -> LoroResult<Self> {
        match v {
            0 => Ok(ContainerType::Map),
            1 => Ok(ContainerType::List),
            2 => Ok(ContainerType::Text),
            3 => Ok(ContainerType::Tree),
            4 => Ok(ContainerType::MovableList),
            #[cfg(feature = "counter")]
            5 => Ok(ContainerType::Counter),
            x => Ok(ContainerType::Unknown(x)),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename = "ContainerType")]
enum ContainerTypeSerdeRepr {
    Text,
    Map,
    List,
    MovableList,
    Tree,
    #[cfg(feature = "counter")]
    Counter,
    Unknown(u8),
}

// For some historical reason, we have another to_byte format for ContainerType,
// it was used for serde of ContainerType
fn historical_container_type_to_byte(c: ContainerType) -> u8 {
    match c {
        ContainerType::Text => 0,
        ContainerType::Map => 1,
        ContainerType::List => 2,
        ContainerType::MovableList => 3,
        ContainerType::Tree => 4,
        #[cfg(feature = "counter")]
        ContainerType::Counter => 5,
        ContainerType::Unknown(k) => k,
    }
}

fn historical_try_byte_to_container(byte: u8) -> ContainerType {
    match byte {
        0 => ContainerType::Text,
        1 => ContainerType::Map,
        2 => ContainerType::List,
        3 => ContainerType::MovableList,
        4 => ContainerType::Tree,
        #[cfg(feature = "counter")]
        5 => ContainerType::Counter,
        _ => ContainerType::Unknown(byte),
    }
}

impl From<ContainerType> for ContainerTypeSerdeRepr {
    fn from(value: ContainerType) -> Self {
        match value {
            ContainerType::Text => Self::Text,
            ContainerType::Map => Self::Map,
            ContainerType::List => Self::List,
            ContainerType::MovableList => Self::MovableList,
            ContainerType::Tree => Self::Tree,
            #[cfg(feature = "counter")]
            ContainerType::Counter => Self::Counter,
            ContainerType::Unknown(value) => Self::Unknown(value),
        }
    }
}

impl From<ContainerTypeSerdeRepr> for ContainerType {
    fn from(value: ContainerTypeSerdeRepr) -> Self {
        match value {
            ContainerTypeSerdeRepr::Text => ContainerType::Text,
            ContainerTypeSerdeRepr::Map => ContainerType::Map,
            ContainerTypeSerdeRepr::List => ContainerType::List,
            ContainerTypeSerdeRepr::MovableList => ContainerType::MovableList,
            ContainerTypeSerdeRepr::Tree => ContainerType::Tree,
            #[cfg(feature = "counter")]
            ContainerTypeSerdeRepr::Counter => ContainerType::Counter,
            ContainerTypeSerdeRepr::Unknown(value) => ContainerType::Unknown(value),
        }
    }
}

impl Serialize for ContainerType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if serializer.is_human_readable() {
            ContainerTypeSerdeRepr::from(*self).serialize(serializer)
        } else {
            serializer.serialize_u8(historical_container_type_to_byte(*self))
        }
    }
}

impl<'de> Deserialize<'de> for ContainerType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            let repr = ContainerTypeSerdeRepr::deserialize(deserializer)?;
            Ok(repr.into())
        } else {
            let value = u8::deserialize(deserializer)?;
            Ok(historical_try_byte_to_container(value))
        }
    }
}

pub type IdSpanVector = rustc_hash::FxHashMap<PeerID, CounterSpan>;

mod container {
    use super::*;

    impl Display for ContainerType {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(match self {
                ContainerType::Map => "Map",
                ContainerType::List => "List",
                ContainerType::MovableList => "MovableList",
                ContainerType::Text => "Text",
                ContainerType::Tree => "Tree",
                #[cfg(feature = "counter")]
                ContainerType::Counter => "Counter",
                ContainerType::Unknown(k) => return f.write_fmt(format_args!("Unknown({k})")),
            })
        }
    }

    impl Display for ContainerID {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                ContainerID::Root {
                    name,
                    container_type,
                } => f.write_fmt(format_args!("cid:root-{name}:{container_type}"))?,
                ContainerID::Normal {
                    peer,
                    counter,
                    container_type,
                } => f.write_fmt(format_args!(
                    "cid:{id}:{container_type}",
                    id = ID::new(*peer, *counter),
                    container_type = container_type
                ))?,
            };
            Ok(())
        }
    }

    impl TryFrom<&str> for ContainerID {
        type Error = ();

        fn try_from(mut s: &str) -> Result<Self, Self::Error> {
            if !s.starts_with("cid:") {
                return Err(());
            }

            s = &s[4..];
            if s.starts_with("root-") {
                // root container
                s = &s[5..];
                let split = s.rfind(':').ok_or(())?;
                if split == 0 {
                    return Err(());
                }
                let kind = ContainerType::try_from(&s[split + 1..]).map_err(|_| ())?;
                let name = &s[..split];
                Ok(ContainerID::Root {
                    name: name.into(),
                    container_type: kind,
                })
            } else {
                let mut iter = s.split(':');
                let id = iter.next().ok_or(())?;
                let kind = iter.next().ok_or(())?;
                if iter.next().is_some() {
                    return Err(());
                }

                let id = ID::try_from(id).map_err(|_| ())?;
                let kind = ContainerType::try_from(kind).map_err(|_| ())?;
                Ok(ContainerID::Normal {
                    peer: id.peer,
                    counter: id.counter,
                    container_type: kind,
                })
            }
        }
    }

    impl ContainerID {
        #[inline]
        pub fn new_normal(id: ID, container_type: ContainerType) -> Self {
            ContainerID::Normal {
                peer: id.peer,
                counter: id.counter,
                container_type,
            }
        }

        #[inline]
        pub fn new_root(name: &str, container_type: ContainerType) -> Self {
            if !check_root_container_name(name) {
                panic!(
                    "Invalid root container name, it should not be empty or contain '/' or '\\0'"
                );
            } else {
                ContainerID::Root {
                    name: name.into(),
                    container_type,
                }
            }
        }

        #[inline]
        pub fn name(&self) -> &InternalString {
            match self {
                ContainerID::Root { name, .. } => name,
                ContainerID::Normal { .. } => unreachable!(),
            }
        }

        #[inline]
        pub fn container_type(&self) -> ContainerType {
            match self {
                ContainerID::Root { container_type, .. } => *container_type,
                ContainerID::Normal { container_type, .. } => *container_type,
            }
        }

        pub fn is_unknown(&self) -> bool {
            matches!(self.container_type(), ContainerType::Unknown(_))
        }

        /// Create a mergeable container ID for the given parent, key, and container type.
        /// The ID is encoded as a Root container with a reserved namespace prefix so that
        /// two peers calling this with the same arguments produce the identical `ContainerID`.
        pub fn new_mergeable(
            parent: &ContainerID,
            key: &str,
            container_type: ContainerType,
        ) -> Self {
            let mut encoded = Vec::new();
            write_len_prefixed_segment(&mut encoded, &parent.to_bytes());
            write_len_prefixed_segment(&mut encoded, key.as_bytes());
            encoded.push(container_type.to_u8());

            let name = format!("{}{}", MERGEABLE_NAMESPACE_PREFIX, hex_encode(&encoded));

            Self::Root {
                name: name.into(),
                container_type,
            }
        }

        /// Returns `true` if this is a mergeable container ID (i.e. created via `new_mergeable`).
        pub fn is_mergeable(&self) -> bool {
            matches!(self, Self::Root { name, .. } if name.starts_with(MERGEABLE_NAMESPACE_PREFIX))
        }

        /// Decode a mergeable container ID back into its `(parent, key, container_type)` components.
        /// Returns `None` if this is not a valid mergeable container ID.
        pub fn parse_mergeable(&self) -> Option<(ContainerID, String, ContainerType)> {
            let Self::Root {
                name,
                container_type,
            } = self
            else {
                return None;
            };
            let payload = name.strip_prefix(MERGEABLE_NAMESPACE_PREFIX)?;
            let decoded = hex_decode(payload)?;
            let mut input = decoded.as_slice();
            let parent_bytes = read_len_prefixed_segment(&mut input)?;
            let key_bytes = read_len_prefixed_segment(&mut input)?;
            if input.len() != 1 {
                return None;
            }
            let encoded_type = ContainerType::try_from_u8(input[0]).ok()?;
            if encoded_type != *container_type {
                return None;
            }
            let key = String::from_utf8(key_bytes).ok()?;
            Some((ContainerID::from_bytes(&parent_bytes), key, encoded_type))
        }
    }

    impl TryFrom<&str> for ContainerType {
        type Error = LoroError;

        fn try_from(value: &str) -> Result<Self, Self::Error> {
            match value {
                "Map" | "map" => Ok(ContainerType::Map),
                "List" | "list" => Ok(ContainerType::List),
                "Text" | "text" => Ok(ContainerType::Text),
                "Tree" | "tree" => Ok(ContainerType::Tree),
                "MovableList" | "movableList" => Ok(ContainerType::MovableList),
                #[cfg(feature = "counter")]
                "Counter" | "counter" => Ok(ContainerType::Counter),
                a => {
                    if a.ends_with(')') {
                        let start = a.find('(').ok_or_else(|| {
                            LoroError::DecodeError(
                                format!("Invalid container type string \"{value}\"").into(),
                            )
                        })?;
                        let k = a[start+1..a.len() - 1].parse().map_err(|_| {
                            LoroError::DecodeError(
                    format!("Unknown container type \"{value}\". The valid options are Map|List|Text|Tree|MovableList.").into(),
                )
                        })?;
                        match ContainerType::try_from_u8(k) {
                            Ok(k) => Ok(k),
                            Err(_) => Ok(ContainerType::Unknown(k)),
                        }
                    } else {
                        Err(LoroError::DecodeError(
                    format!("Unknown container type \"{value}\". The valid options are Map|List|Text|Tree|MovableList.").into(),
                ))
                    }
                }
            }
        }
    }
}

/// In movable tree, we use a specific [`TreeID`] to represent the root of **ALL** deleted tree node.
///
/// Deletion operation is equivalent to move target tree node to [`DELETED_TREE_ROOT`].
pub const DELETED_TREE_ROOT: TreeID = TreeID {
    peer: PeerID::MAX,
    counter: Counter::MAX,
};

/// Each node of movable tree has a unique [`TreeID`] generated by Loro.
///
/// To further represent the metadata (a MapContainer) associated with each node,
/// we also use [`TreeID`] as [`ID`] portion of [`ContainerID`].
/// This not only allows for convenient association of metadata with each node,
/// but also ensures the uniqueness of the MapContainer.
///
/// Special ID:
/// - [`DELETED_TREE_ROOT`]: the root of all deleted nodes. To get it by [`TreeID::delete_root()`]
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]

pub struct TreeID {
    pub peer: PeerID,
    // TODO: can use a NonMax here
    pub counter: Counter,
}

impl TreeID {
    #[inline(always)]
    pub fn new(peer: PeerID, counter: Counter) -> Self {
        Self { peer, counter }
    }

    /// return [`DELETED_TREE_ROOT`]
    pub const fn delete_root() -> Self {
        DELETED_TREE_ROOT
    }

    /// return `true` if the `TreeID` is deleted root
    pub fn is_deleted_root(&self) -> bool {
        self == &DELETED_TREE_ROOT
    }

    pub fn from_id(id: ID) -> Self {
        Self {
            peer: id.peer,
            counter: id.counter,
        }
    }

    pub fn id(&self) -> ID {
        ID {
            peer: self.peer,
            counter: self.counter,
        }
    }

    pub fn associated_meta_container(&self) -> ContainerID {
        ContainerID::new_normal(self.id(), ContainerType::Map)
    }
}

impl Display for TreeID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.id().fmt(f)
    }
}

impl TryFrom<&str> for TreeID {
    type Error = LoroError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let id = ID::try_from(value)?;
        Ok(TreeID {
            peer: id.peer,
            counter: id.counter,
        })
    }
}

#[cfg(feature = "wasm")]
pub mod wasm {
    use crate::{LoroError, TreeID};
    use wasm_bindgen::JsValue;
    impl From<TreeID> for JsValue {
        fn from(value: TreeID) -> Self {
            JsValue::from_str(&format!("{value}"))
        }
    }

    impl TryFrom<JsValue> for TreeID {
        type Error = LoroError;
        fn try_from(value: JsValue) -> Result<Self, Self::Error> {
            let id = value.as_string().unwrap();
            TreeID::try_from(id.as_str())
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{
        mergeable_discriminator, mergeable_discriminator_string, parse_mergeable_discriminator,
        ContainerID, ContainerType, LoroValue, ID,
    };

    #[test]
    fn mergeable_discriminator_string_round_trips_every_kind() {
        let kinds = [
            ContainerType::Map,
            ContainerType::List,
            ContainerType::Text,
            ContainerType::Tree,
            ContainerType::MovableList,
            #[cfg(feature = "counter")]
            ContainerType::Counter,
        ];
        for kind in kinds {
            let s = mergeable_discriminator_string(kind);
            // The string reuses the reserved mergeable namespace prefix so older
            // clients that don't understand it at least see it as a sentinel.
            assert!(
                s.starts_with(crate::MERGEABLE_NAMESPACE_PREFIX),
                "discriminator {s:?} must start with the mergeable namespace prefix"
            );
            let value = mergeable_discriminator(kind);
            assert_eq!(value, LoroValue::String(s.clone().into()));
            assert_eq!(
                parse_mergeable_discriminator(&value),
                Some(kind),
                "discriminator value {value:?} must parse back to {kind:?}"
            );
        }
    }

    #[test]
    fn mergeable_discriminator_exact_bytes() {
        // The exact shape agreed upon in loro-dev/loro#759: `LoroValue::String("🤝:Map")`.
        assert_eq!(
            mergeable_discriminator(ContainerType::Map),
            LoroValue::String("🤝:Map".into())
        );
        assert_eq!(
            mergeable_discriminator(ContainerType::List),
            LoroValue::String("🤝:List".into())
        );
    }

    #[test]
    fn parse_mergeable_discriminator_rejects_non_discriminators() {
        // Plain user strings, even ones that look map-ish, are not discriminators.
        assert_eq!(
            parse_mergeable_discriminator(&LoroValue::String("Map".into())),
            None
        );
        assert_eq!(
            parse_mergeable_discriminator(&LoroValue::String("hello".into())),
            None
        );
        // The bare prefix with no/unknown kind is not a valid discriminator.
        assert_eq!(
            parse_mergeable_discriminator(&LoroValue::String("🤝:".into())),
            None
        );
        assert_eq!(
            parse_mergeable_discriminator(&LoroValue::String("🤝:NotAKind".into())),
            None
        );
        // Non-string values are never discriminators.
        assert_eq!(parse_mergeable_discriminator(&LoroValue::Double(1.0)), None);
        assert_eq!(parse_mergeable_discriminator(&LoroValue::Null), None);
    }

    #[test]
    fn test_container_id_convert_to_and_from_str() {
        let id = ContainerID::Root {
            name: "name".into(),
            container_type: crate::ContainerType::Map,
        };
        let id_str = id.to_string();
        assert_eq!(id_str.as_str(), "cid:root-name:Map");
        assert_eq!(ContainerID::try_from(id_str.as_str()).unwrap(), id);

        let id = ContainerID::Normal {
            counter: 10,
            peer: 255,
            container_type: crate::ContainerType::Map,
        };
        let id_str = id.to_string();
        assert_eq!(id_str.as_str(), "cid:10@255:Map");
        assert_eq!(ContainerID::try_from(id_str.as_str()).unwrap(), id);

        let id = ContainerID::try_from("cid:root-a:b:c:Tree").unwrap();
        assert_eq!(
            id,
            ContainerID::new_root("a:b:c", crate::ContainerType::Tree)
        );
    }

    #[test]
    fn test_convert_invalid_container_id_str() {
        assert!(ContainerID::try_from("cid:root-:Map").is_err());
        assert!(ContainerID::try_from("cid:0@:Map").is_err());
        assert!(ContainerID::try_from("cid:@:Map").is_err());
        assert!(ContainerID::try_from("cid:x@0:Map").is_err());
        assert!(ContainerID::try_from("id:0@0:Map").is_err());
        assert!(ContainerID::try_from("cid:0@0:Unknown(6)").is_ok());
    }

    #[test]
    fn test_container_id_encode_and_decode() {
        let id = ContainerID::new_normal(ID::new(1, 2), ContainerType::Map);
        let bytes = id.to_bytes();
        assert_eq!(ContainerID::from_bytes(&bytes), id);

        let id = ContainerID::new_normal(ID::new(u64::MAX, i32::MAX), ContainerType::Text);
        let bytes = id.to_bytes();
        assert_eq!(ContainerID::from_bytes(&bytes), id);

        let id = ContainerID::new_root("test_root", ContainerType::List);
        let bytes = id.to_bytes();
        assert_eq!(ContainerID::from_bytes(&bytes), id);

        let id = ContainerID::new_normal(ID::new(0, 0), ContainerType::MovableList);
        let bytes = id.to_bytes();
        assert_eq!(ContainerID::from_bytes(&bytes), id);

        let id = ContainerID::new_root(&"x".repeat(1024), ContainerType::Tree);
        let bytes = id.to_bytes();
        assert_eq!(ContainerID::from_bytes(&bytes), id);

        #[cfg(feature = "counter")]
        {
            let id = ContainerID::new_normal(ID::new(42, 100), ContainerType::Counter);
            let bytes = id.to_bytes();
            assert_eq!(ContainerID::from_bytes(&bytes), id);
        }

        let id = ContainerID::new_normal(ID::new(1, 1), ContainerType::Unknown(100));
        let bytes = id.to_bytes();
        assert_eq!(ContainerID::from_bytes(&bytes), id);
    }

    #[test]
    fn container_id_try_from_bytes_rejects_trailing_bytes() {
        let normal = ContainerID::new_normal(ID::new(1, 2), ContainerType::Map);
        let mut normal_bytes = normal.to_bytes();
        normal_bytes.push(0);
        assert!(ContainerID::try_from_bytes(&normal_bytes).is_err());

        let root = ContainerID::new_root("root", ContainerType::List);
        let mut root_bytes = root.to_bytes();
        root_bytes.push(0);
        assert!(ContainerID::try_from_bytes(&root_bytes).is_err());
    }
}
