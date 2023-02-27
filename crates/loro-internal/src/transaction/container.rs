use crate::{container::registry::ContainerIdx, ContainerType, LoroError, Prelim};

use super::{op::TransactionOp, Transaction};

pub enum TransactionalContainer {
    List(TransactionalList),
}

impl TransactionalContainer {
    pub fn idx(&self) -> ContainerIdx {
        match &self {
            Self::List(list) => list.idx(),
        }
    }
}

impl TransactionalContainer {
    pub(super) fn new(type_: ContainerType, idx: ContainerIdx) -> Self {
        match type_ {
            ContainerType::List => Self::List(TransactionalList(idx)),
            _ => unimplemented!(),
        }
    }
}

pub struct TransactionalList(ContainerIdx);

impl TransactionalList {
    pub fn idx(&self) -> ContainerIdx {
        self.0
    }

    pub fn insert<P: Prelim>(
        &self,
        txn: &mut Transaction,
        pos: usize,
        value: P,
    ) -> Result<Option<TransactionalContainer>, LoroError> {
        let (value, maybe_container) = value.convert_value()?;
        if let Some(prelim) = maybe_container {
            let container = txn
                .insert(TransactionOp::insert_list_container(
                    self.0,
                    pos,
                    value.into_container().unwrap(),
                ))?
                .unwrap();
            prelim.integrate(txn, container.idx());
            Ok(Some(container))
        } else {
            let value = value.into_value().unwrap();
            txn.insert(TransactionOp::insert_list_value(self.0, pos, value))?;
            Ok(None)
        }
    }
}
