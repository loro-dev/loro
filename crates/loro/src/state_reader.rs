//! Field-level reads under one document lock. See [`LoroDoc::read_state`].

use std::marker::PhantomData;

use loro_internal::container::idx::ContainerIdx;
use loro_internal::handler::HandlerTrait;

use crate::{ContainerID, ContainerTrait, ContainerType, InternalString, LoroDoc, LoroValue};

/// Makes `'r` invariant, so references from two `read_state` calls cannot be mixed.
type Brand<'r> = PhantomData<fn(&'r ()) -> &'r ()>;

/// A container of the document being read, valid only inside one
/// [`LoroDoc::read_state`] call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ContainerRef<'r> {
    idx: ContainerIdx,
    _brand: Brand<'r>,
}

impl ContainerRef<'_> {
    /// The type of the container.
    pub fn container_type(&self) -> ContainerType {
        self.idx.get_type()
    }
}

/// One slot of a map or list read through a [`StateReader`].
#[derive(Clone, Debug)]
pub enum ReadEntry<'r> {
    /// A value. Never `LoroValue::Container`: child containers are [`ReadEntry::Container`].
    Value(LoroValue),
    /// A child container, read further through the same reader.
    Container(ContainerRef<'r>),
}

/// Reads the document state field by field while holding the state lock once.
///
/// Unlike `get_deep_value`, nothing is materialized beyond what is asked for: a map read
/// returns one value or one child [`ContainerRef`]. Mergeable children resolve like
/// `LoroMap::get` does.
#[derive(Debug)]
pub struct StateReader<'r> {
    inner: loro_internal::StateReader<'r>,
    _brand: Brand<'r>,
}

impl<'r> StateReader<'r> {
    fn wrap(&self, idx: ContainerIdx) -> ContainerRef<'r> {
        ContainerRef {
            idx,
            _brand: PhantomData,
        }
    }

    fn entry(&self, e: loro_internal::ReadEntry) -> ReadEntry<'r> {
        match e {
            loro_internal::ReadEntry::Value(v) => ReadEntry::Value(v),
            loro_internal::ReadEntry::Container(idx) => ReadEntry::Container(self.wrap(idx)),
        }
    }

    /// The reference for a container handle of this document. `None` if it is detached or
    /// belongs to another document.
    pub fn container<C: ContainerTrait>(&self, c: &C) -> Option<ContainerRef<'r>>
    where
        C::Handler: HandlerTrait,
    {
        self.inner
            .container_of(&c.to_handler())
            .map(|i| self.wrap(i))
    }

    /// The reference for a container id. Like getting a handler by id, it does not check that
    /// the container exists or is alive.
    pub fn container_by_id(&self, id: &ContainerID) -> ContainerRef<'r> {
        self.wrap(self.inner.container_by_id(id))
    }

    /// The id of a container.
    pub fn container_id(&self, c: ContainerRef<'r>) -> ContainerID {
        self.inner.container_id(c.idx)
    }

    /// Whether the container is deleted, like [`ContainerTrait::is_deleted`].
    pub fn is_deleted(&mut self, c: ContainerRef<'r>) -> bool {
        self.inner.is_deleted(c.idx)
    }

    /// The entry at `key` of a map, like `LoroMap::get`.
    ///
    /// # Panics
    ///
    /// If `map` is not a map.
    pub fn map_get(&mut self, map: ContainerRef<'r>, key: &str) -> Option<ReadEntry<'r>> {
        assert_eq!(map.container_type(), ContainerType::Map);
        let e = self.inner.map_get(map.idx, key)?;
        Some(self.entry(e))
    }

    /// The number of entries of a map.
    pub fn map_len(&mut self, map: ContainerRef<'r>) -> usize {
        self.inner.map_len(map.idx)
    }

    /// The keys of a map.
    pub fn map_keys(&mut self, map: ContainerRef<'r>) -> Vec<InternalString> {
        self.inner.map_keys(map.idx)
    }

    /// The length of a List or MovableList.
    pub fn list_len(&mut self, list: ContainerRef<'r>) -> usize {
        self.inner.list_len(list.idx)
    }

    /// The element at `index` of a List or MovableList.
    pub fn list_get(&mut self, list: ContainerRef<'r>, index: usize) -> Option<ReadEntry<'r>> {
        let e = self.inner.list_get(list.idx, index)?;
        Some(self.entry(e))
    }

    /// All elements of a List or MovableList.
    pub fn list_entries(&mut self, list: ContainerRef<'r>) -> Vec<ReadEntry<'r>> {
        self.inner
            .list_entries(list.idx)
            .into_iter()
            .map(|e| self.entry(e))
            .collect()
    }

    /// The shallow value: a Text as a string, a Counter as a number, children of maps and
    /// lists as `LoroValue::Container`.
    pub fn value(&mut self, c: ContainerRef<'r>) -> LoroValue {
        self.inner.value(c.idx)
    }

    /// The deep value, like the handler's `get_deep_value`.
    pub fn deep_value(&mut self, c: ContainerRef<'r>) -> LoroValue {
        self.inner.deep_value(c.idx)
    }
}

impl LoroDoc {
    /// Read the current state field by field while holding the document's state lock once.
    ///
    /// This is the fast path for converting part of the document into application types:
    /// unlike `get_deep_value`, it materializes only what `f` reads. Reads see uncommitted
    /// local changes, like handler reads.
    ///
    /// `f` must not call into this document (handlers, `commit`, `import`, ...): the state lock
    /// is held, so that would deadlock (or panic in debug builds). [`ContainerRef`]s cannot
    /// leave `f` or be used with another call's reader.
    ///
    /// # Example
    /// ```
    /// use loro::{LoroDoc, LoroMap, ReadEntry};
    /// let doc = LoroDoc::new();
    /// let todo = doc.get_map("todo");
    /// todo.insert("title", "write docs").unwrap();
    /// let title = doc.read_state(|r| {
    ///     let map = r.container(&todo).unwrap();
    ///     match r.map_get(map, "title") {
    ///         Some(ReadEntry::Value(v)) => v.into_string().ok().map(|s| s.to_string()),
    ///         _ => None,
    ///     }
    /// });
    /// assert_eq!(title.as_deref(), Some("write docs"));
    /// ```
    ///
    /// A [`ContainerRef`] cannot leave the closure:
    ///
    /// ```compile_fail
    /// # use loro::LoroDoc;
    /// let doc = LoroDoc::new();
    /// let map = doc.get_map("m");
    /// let leaked = doc.read_state(|r| r.container(&map));
    /// ```
    ///
    /// nor be used with another call's reader:
    ///
    /// ```compile_fail
    /// # use loro::LoroDoc;
    /// let (a, b) = (LoroDoc::new(), LoroDoc::new());
    /// let map = a.get_map("m");
    /// a.read_state(|ra| {
    ///     let m = ra.container(&map).unwrap();
    ///     b.read_state(|rb| rb.map_len(m));
    /// });
    /// ```
    pub fn read_state<R>(&self, f: impl for<'r> FnOnce(&mut StateReader<'r>) -> R) -> R {
        let mut state = self.doc.app_state().lock();
        let mut reader = StateReader {
            inner: loro_internal::StateReader::new(&mut state),
            _brand: PhantomData,
        };
        f(&mut reader)
    }
}
