//! A [`Slot`] is the place a value is written to: a map key, a list position,
//! or an existing container. `ToLoro` impls only talk to slots, so one impl
//! covers struct fields, list elements and model roots.

use loro::{
    Container, ContainerTrait, ContainerType, LoroCounter, LoroList, LoroMap, LoroMovableList,
    LoroText, LoroValue, ValueOrContainer,
};

use crate::{ErrorKind, ModelError, Result};

/// A list container that may or may not support `set`/`mov`.
#[derive(Clone, Debug)]
pub enum ListRef {
    List(LoroList),
    Movable(LoroMovableList),
}

impl ListRef {
    pub fn from_container(c: &Container) -> Option<Self> {
        match c {
            Container::List(l) => Some(Self::List(l.clone())),
            Container::MovableList(l) => Some(Self::Movable(l.clone())),
            _ => None,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::List(l) => l.len(),
            Self::Movable(l) => l.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, index: usize) -> Option<ValueOrContainer> {
        match self {
            Self::List(l) => l.get(index),
            Self::Movable(l) => l.get(index),
        }
    }

    pub fn delete(&self, pos: usize, len: usize) -> Result<()> {
        match self {
            Self::List(l) => l.delete(pos, len)?,
            Self::Movable(l) => l.delete(pos, len)?,
        }
        Ok(())
    }

    pub fn to_container(&self) -> Container {
        match self {
            Self::List(l) => l.to_container(),
            Self::Movable(l) => l.to_container(),
        }
    }

    pub fn get_deep_value(&self) -> LoroValue {
        match self {
            Self::List(l) => l.get_deep_value(),
            Self::Movable(l) => l.get_deep_value(),
        }
    }

    /// Move an element. A plain `LoroList` has no move op, so this falls back
    /// to delete + re-insert of the deep value, which loses element identity.
    pub fn mov(&self, from: usize, to: usize) -> Result<()> {
        match self {
            Self::Movable(l) => l.mov(from, to)?,
            Self::List(l) => {
                let v = l
                    .get(from)
                    .ok_or_else(|| ModelError::new(ErrorKind::Missing).in_index(from))?
                    .get_deep_value();
                l.delete(from, 1)?;
                l.insert(to, v)?;
            }
        }
        Ok(())
    }
}

pub enum Slot<'a> {
    /// Reconcile into this container itself (a model root or a resolved view).
    Container(&'a Container),
    MapKey(&'a LoroMap, &'a str),
    /// An existing list element.
    ListAt(&'a ListRef, usize),
    /// A new list element inserted before `index`.
    ListInsert(&'a ListRef, usize),
}

impl Slot<'_> {
    pub fn current(&self) -> Option<ValueOrContainer> {
        match self {
            Slot::Container(c) => Some(ValueOrContainer::Container((*c).clone())),
            Slot::MapKey(m, k) => m.get(k),
            Slot::ListAt(l, i) => l.get(*i),
            Slot::ListInsert(..) => None,
        }
    }

    /// Write a scalar value. Writing the value already stored is a no-op, so
    /// reconciling an unchanged input emits no ops.
    pub fn set_value(self, v: LoroValue) -> Result<()> {
        if let Some(ValueOrContainer::Value(cur)) = self.current() {
            if cur == v {
                return Ok(());
            }
        }
        match self {
            Slot::Container(c) => Err(ModelError::mismatch("value", c.get_type())),
            Slot::MapKey(m, k) => Ok(m.insert(k, v)?),
            Slot::ListAt(ListRef::Movable(l), i) => Ok(l.set(i, v)?),
            Slot::ListAt(ListRef::List(l), i) => {
                l.delete(i, 1)?;
                Ok(l.insert(i, v)?)
            }
            Slot::ListInsert(ListRef::Movable(l), i) => Ok(l.insert(i, v)?),
            Slot::ListInsert(ListRef::List(l), i) => Ok(l.insert(i, v)?),
        }
    }

    /// Delete the value at this slot (used for `None` in map fields).
    pub fn delete(self) -> Result<()> {
        match self {
            Slot::MapKey(m, k) => {
                if m.get(k).is_some() {
                    m.delete(k)?;
                }
                Ok(())
            }
            other => other.set_value(LoroValue::Null),
        }
    }

    /// Return the container at this slot, reusing the existing one when its
    /// type matches and creating (or replacing) it otherwise.
    ///
    /// New children under a map key are mergeable containers, so two peers
    /// that initialize the same field concurrently converge to one container
    /// instead of LWW-dropping one side's subtree.
    pub fn ensure_container(self, ty: ContainerType) -> Result<Container> {
        if let Some(ValueOrContainer::Container(c)) = self.current() {
            if c.get_type() == ty {
                return Ok(c);
            }
        }
        match self {
            Slot::Container(c) => Err(ModelError::mismatch(type_name(ty), c.get_type())),
            Slot::MapKey(m, k) => {
                if m.get(k).is_some() {
                    // `ensure_mergeable_*` refuses to overwrite user values.
                    m.delete(k)?;
                }
                ensure_mergeable(m, k, ty)
            }
            Slot::ListAt(list, i) => match list {
                ListRef::Movable(l) => set_container(l, i, ty),
                ListRef::List(l) => {
                    l.delete(i, 1)?;
                    insert_list_container(l, i, ty)
                }
            },
            Slot::ListInsert(list, i) => match list {
                ListRef::Movable(l) => insert_movable_container(l, i, ty),
                ListRef::List(l) => insert_list_container(l, i, ty),
            },
        }
    }

    pub fn ensure_map(self) -> Result<LoroMap> {
        Ok(self
            .ensure_container(ContainerType::Map)?
            .into_map()
            .unwrap())
    }

    pub fn ensure_text(self) -> Result<LoroText> {
        Ok(self
            .ensure_container(ContainerType::Text)?
            .into_text()
            .unwrap())
    }

    pub fn ensure_counter(self) -> Result<LoroCounter> {
        Ok(self
            .ensure_container(ContainerType::Counter)?
            .into_counter()
            .unwrap())
    }

    /// Like [`Slot::ensure_container`], but accepts either list kind and
    /// only creates `prefer` when neither exists.
    pub fn ensure_list(self, prefer: ContainerType) -> Result<ListRef> {
        if let Some(ValueOrContainer::Container(c)) = self.current() {
            if let Some(l) = ListRef::from_container(&c) {
                return Ok(l);
            }
        }
        Ok(ListRef::from_container(&self.ensure_container(prefer)?).unwrap())
    }
}

fn type_name(ty: ContainerType) -> &'static str {
    match ty {
        ContainerType::Map => "map",
        ContainerType::List => "list",
        ContainerType::MovableList => "movable list",
        ContainerType::Text => "text",
        ContainerType::Counter => "counter",
        ContainerType::Tree => "tree",
        _ => "container",
    }
}

fn ensure_mergeable(m: &LoroMap, k: &str, ty: ContainerType) -> Result<Container> {
    Ok(match ty {
        ContainerType::Map => m.ensure_mergeable_map(k)?.to_container(),
        ContainerType::List => m.ensure_mergeable_list(k)?.to_container(),
        ContainerType::MovableList => m.ensure_mergeable_movable_list(k)?.to_container(),
        ContainerType::Text => m.ensure_mergeable_text(k)?.to_container(),
        ContainerType::Counter => m.ensure_mergeable_counter(k)?.to_container(),
        ContainerType::Tree => m.ensure_mergeable_tree(k)?.to_container(),
        _ => return Err(ModelError::mismatch("known container type", ty)),
    })
}

macro_rules! by_type {
    ($ty:expr, |$c:ident| $body:expr) => {
        match $ty {
            ContainerType::Map => {
                let $c = LoroMap::new();
                $body.to_container()
            }
            ContainerType::List => {
                let $c = LoroList::new();
                $body.to_container()
            }
            ContainerType::MovableList => {
                let $c = LoroMovableList::new();
                $body.to_container()
            }
            ContainerType::Text => {
                let $c = LoroText::new();
                $body.to_container()
            }
            ContainerType::Counter => {
                let $c = LoroCounter::new();
                $body.to_container()
            }
            ty => return Err(ModelError::mismatch("supported container type", ty)),
        }
    };
}

fn set_container(l: &LoroMovableList, i: usize, ty: ContainerType) -> Result<Container> {
    Ok(by_type!(ty, |c| l.set_container(i, c)?))
}

fn insert_movable_container(l: &LoroMovableList, i: usize, ty: ContainerType) -> Result<Container> {
    Ok(by_type!(ty, |c| l.insert_container(i, c)?))
}

fn insert_list_container(l: &LoroList, i: usize, ty: ContainerType) -> Result<Container> {
    Ok(by_type!(ty, |c| l.insert_container(i, c)?))
}
