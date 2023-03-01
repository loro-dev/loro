use std::sync::{Mutex, Weak};

use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;

use crate::{
    container::registry::ContainerIdx,
    transaction::{op::TransactionOp, Transaction},
    ContainerType, LoroError, LoroValue,
};

/// Prelim is a value that is not yet integrated into the Loro.
pub trait Prelim: Sized {
    /// Convert the value into a [`PrelimValue`].
    /// If the value is preliminary(container-like), return [`PrelimValue::Container`] and `Some(self)`
    /// that means the value needs to be integrated into the Loro by creating another container.
    ///
    /// If the value is not preliminary, return [`PrelimValue::Value`] and `None`. The value will be insert into the container of Loro directly.
    fn convert_value(self) -> Result<(PrelimValue, Option<Self>), LoroError>;

    /// How to integrate the value into the Loro.
    fn integrate(self, txn: &mut Transaction, container_idx: ContainerIdx)
        -> Result<(), LoroError>;
}

#[derive(Debug, EnumAsInner)]
pub enum PrelimValue {
    Value(LoroValue),
    Container(ContainerType),
}

impl<T> Prelim for T
where
    T: Into<LoroValue>,
{
    fn convert_value(self) -> Result<(PrelimValue, Option<Self>), LoroError> {
        let value: LoroValue = self.into();
        if let LoroValue::Unresolved(_) = value {
            return Err(LoroError::PrelimError);
        }
        Ok((PrelimValue::Value(value), None))
    }

    fn integrate(self, txn: &mut Transaction, container: ContainerIdx) -> Result<(), LoroError> {
        Ok(())
    }
}

impl Prelim for ContainerType {
    fn convert_value(self) -> Result<(PrelimValue, Option<Self>), LoroError> {
        Ok((PrelimValue::Container(self), Some(self)))
    }

    fn integrate(self, txn: &mut Transaction, container: ContainerIdx) -> Result<(), LoroError> {
        Ok(())
    }
}

impl From<LoroValue> for PrelimValue {
    fn from(value: LoroValue) -> Self {
        PrelimValue::Value(value)
    }
}

impl From<ContainerType> for PrelimValue {
    fn from(container: ContainerType) -> Self {
        PrelimValue::Container(container)
    }
}

impl From<i32> for PrelimValue {
    fn from(v: i32) -> Self {
        PrelimValue::Value(v.into())
    }
}

impl From<f64> for PrelimValue {
    fn from(v: f64) -> Self {
        PrelimValue::Value(v.into())
    }
}

// #[derive(Debug)]
// pub struct PrelimText(String);

// impl Prelim for PrelimText {
//     fn convert_value(self) -> Result<(PrelimValue, Option<Self>), LoroError> {
//         Ok((PrelimValue::Container(ContainerType::Text), Some(self)))
//     }

//     fn integrate<C: Context>(
//         self,
//         ctx: &C,
//         container: Weak<Mutex<ContainerInstance>>,
//     ) -> Result<(), LoroError> {
//         let text = container.upgrade().unwrap();
//         let mut text = text.try_lock().unwrap();
//         let text = text.as_text_mut().unwrap();
//         text.insert(ctx, 0, &self.0);
//         Ok(())
//     }
// }

#[derive(Debug)]
pub struct PrelimList(Vec<LoroValue>);

impl Prelim for PrelimList {
    fn convert_value(self) -> Result<(PrelimValue, Option<Self>), LoroError> {
        Ok((PrelimValue::Container(ContainerType::List), Some(self)))
    }

    fn integrate(
        self,
        txn: &mut Transaction,
        container_idx: ContainerIdx,
    ) -> Result<(), LoroError> {
        for (i, value) in self.0.into_iter().enumerate() {
            txn.push(
                TransactionOp::insert_list_value(container_idx, i, value),
                None,
            )?;
        }
        Ok(())
    }
}

// #[derive(Debug)]
// pub struct PrelimMap(FxHashMap<String, LoroValue>);

// impl Prelim for PrelimMap {
//     fn convert_value(self) -> Result<(PrelimValue, Option<Self>), LoroError> {
//         Ok((PrelimValue::Container(ContainerType::Map), Some(self)))
//     }

//     fn integrate<C: Context>(
//         self,
//         ctx: &C,
//         container: Weak<Mutex<ContainerInstance>>,
//     ) -> Result<(), LoroError> {
//         let map = container.upgrade().unwrap();
//         let mut map = map.try_lock().unwrap();
//         let map = map.as_map_mut().unwrap();
//         for (key, value) in self.0.into_iter() {
//             map.insert(ctx, key.into(), value)?;
//         }
//         Ok(())
//     }
// }

#[derive(Debug)]
pub enum PrelimContainer {
    // Text(PrelimText),
    // Map(PrelimMap),
    List(PrelimList),
}

// impl Prelim for PrelimContainer {
//     fn convert_value(self) -> Result<(PrelimValue, Option<Self>), LoroError> {
//         match self {
//             PrelimContainer::List(p) => p
//                 .convert_value()
//                 .map(|(v, s)| (v, s.map(PrelimContainer::List))),
//             PrelimContainer::Text(p) => p
//                 .convert_value()
//                 .map(|(v, s)| (v, s.map(PrelimContainer::Text))),
//             PrelimContainer::Map(p) => p
//                 .convert_value()
//                 .map(|(v, s)| (v, s.map(PrelimContainer::Map))),
//         }
//     }

//     fn integrate<C: Context>(
//         self,
//         ctx: &C,
//         container: Weak<Mutex<ContainerInstance>>,
//     ) -> Result<(), LoroError> {
//         match self {
//             PrelimContainer::List(p) => p.integrate(ctx, container),
//             PrelimContainer::Text(p) => p.integrate(ctx, container),
//             PrelimContainer::Map(p) => p.integrate(ctx, container),
//         }
//     }
// }

// impl From<String> for PrelimContainer {
//     fn from(value: String) -> Self {
//         PrelimContainer::Text(PrelimText(value))
//     }
// }

impl From<Vec<LoroValue>> for PrelimContainer {
    fn from(value: Vec<LoroValue>) -> Self {
        PrelimContainer::List(PrelimList(value))
    }
}

// impl From<FxHashMap<String, LoroValue>> for PrelimContainer {
//     fn from(value: FxHashMap<String, LoroValue>) -> Self {
//         PrelimContainer::Map(PrelimMap(value))
//     }
// }
