use std::sync::Arc;

use loro::{ContainerTrait, LoroError, LoroResult, LoroTreeError, TreeID, ID};

use crate::{ContainerID, LoroDoc, LoroValue};

use super::LoroMap;

pub enum TreeParentId {
    Node { id: TreeID },
    Root,
    Deleted,
    Unexist,
}

#[derive(Debug, Clone)]
pub struct LoroTree {
    pub(crate) inner: loro::LoroTree,
}

impl LoroTree {
    pub fn new() -> Self {
        Self {
            inner: loro::LoroTree::new(),
        }
    }

    /// Whether the container is attached to a document
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn is_attached(&self) -> bool {
        self.inner.is_attached()
    }

    /// If a detached container is attached, this method will return its corresponding attached handler.
    pub fn get_attached(&self) -> Option<Arc<Self>> {
        self.inner
            .get_attached()
            .map(|x| Arc::new(Self { inner: x }))
    }

    /// Create a new tree node and return the [`TreeID`].
    ///
    /// If the `parent` is `None`, the created node is the root of a tree.
    /// Otherwise, the created node is a child of the parent tree node.
    ///
    /// # Example
    ///
    /// ```rust
    /// use loro::LoroDoc;
    ///
    /// let doc = LoroDoc::new();
    /// let tree = doc.get_tree("tree");
    /// // create a root
    /// let root = tree.create(None).unwrap();
    /// // create a new child
    /// let child = tree.create(root).unwrap();
    /// ```
    pub fn create(&self, parent: TreeParentId) -> LoroResult<TreeID> {
        self.inner.create(parent)
    }

    /// Create a new tree node at the given index and return the [`TreeID`].
    ///
    /// If the `parent` is `None`, the created node is the root of a tree.
    /// If the `index` is greater than the number of children of the parent, error will be returned.
    pub fn create_at(&self, parent: TreeParentId, index: u32) -> LoroResult<TreeID> {
        self.inner.create_at(parent, index as usize)
    }

    pub fn roots(&self) -> Vec<TreeID> {
        self.inner.roots()
    }

    /// Move the `target` node to be a child of the `parent` node.
    ///
    /// If the `parent` is `None`, the `target` node will be a root.
    ///
    /// # Example
    ///
    /// ```rust
    /// use loro::LoroDoc;
    ///
    /// let doc = LoroDoc::new();
    /// let tree = doc.get_tree("tree");
    /// let root = tree.create(None).unwrap();
    /// let root2 = tree.create(None).unwrap();
    /// // move `root2` to be a child of `root`.
    /// tree.mov(root2, root).unwrap();
    /// ```
    pub fn mov(&self, target: TreeID, parent: TreeParentId) -> LoroResult<()> {
        self.inner.mov(target, parent)
    }

    /// Move the `target` node to be a child of the `parent` node at the given index.
    /// If the `parent` is `None`, the `target` node will be a root.
    pub fn mov_to(&self, target: TreeID, parent: TreeParentId, to: u32) -> LoroResult<()> {
        self.inner.mov_to(target, parent, to as usize)
    }

    /// Move the `target` node to be a child after the `after` node with the same parent.
    pub fn mov_after(&self, target: TreeID, after: TreeID) -> LoroResult<()> {
        self.inner.mov_after(target, after)
    }

    /// Move the `target` node to be a child before the `before` node with the same parent.
    pub fn mov_before(&self, target: TreeID, before: TreeID) -> LoroResult<()> {
        self.inner.mov_before(target, before)
    }

    /// Delete a tree node.
    ///
    /// Note: If the deleted node has children, the children do not appear in the state
    /// rather than actually being deleted.
    ///
    /// # Example
    ///
    /// ```rust
    /// use loro::LoroDoc;
    ///
    /// let doc = LoroDoc::new();
    /// let tree = doc.get_tree("tree");
    /// let root = tree.create(None).unwrap();
    /// tree.delete(root).unwrap();
    /// ```
    pub fn delete(&self, target: TreeID) -> LoroResult<()> {
        self.inner.delete(target)
    }

    /// Get the associated metadata map handler of a tree node.
    ///
    /// # Example
    /// ```rust
    /// use loro::LoroDoc;
    ///
    /// let doc = LoroDoc::new();
    /// let tree = doc.get_tree("tree");
    /// let root = tree.create(None).unwrap();
    /// let root_meta = tree.get_meta(root).unwrap();
    /// root_meta.insert("color", "red");
    /// ```
    pub fn get_meta(&self, target: TreeID) -> LoroResult<Arc<LoroMap>> {
        self.inner
            .get_meta(target)
            .map(|h| Arc::new(LoroMap { inner: h }))
    }

    /// Return the parent of target node.
    ///
    /// - If the target node does not exist, throws Error.
    /// - If the target node is a root node, return `None`.
    pub fn parent(&self, target: TreeID) -> LoroResult<TreeParentId> {
        if let Some(p) = self.inner.parent(target) {
            Ok(p.into())
        } else {
            Err(LoroError::TreeError(LoroTreeError::TreeNodeNotExist(
                target,
            )))
        }
    }

    /// Return whether target node exists.
    pub fn contains(&self, target: TreeID) -> bool {
        self.inner.contains(target)
    }

    /// Return whether target node is deleted.
    ///
    /// # Errors
    ///
    /// - If the target node does not exist, return `LoroTreeError::TreeNodeNotExist`.
    pub fn is_node_deleted(&self, target: TreeID) -> LoroResult<bool> {
        self.inner.is_node_deleted(&target)
    }

    /// Return all nodes, including deleted nodes
    pub fn nodes(&self) -> Vec<TreeID> {
        self.inner.nodes()
    }

    /// Return all children of the target node.
    ///
    /// If the parent node does not exist, return `None`.
    pub fn children(&self, parent: TreeParentId) -> Option<Vec<TreeID>> {
        self.inner.children(parent)
    }

    /// Return the number of children of the target node.
    pub fn children_num(&self, parent: TreeParentId) -> Option<u32> {
        self.inner.children_num(parent).map(|v| v as u32)
    }

    /// Return container id of the tree.
    pub fn id(&self) -> ContainerID {
        self.inner.id().into()
    }

    /// Return the fractional index of the target node with hex format.
    pub fn fractional_index(&self, target: TreeID) -> Option<String> {
        self.inner.fractional_index(target)
    }

    /// Return the flat array of the forest.
    ///
    /// Note: the metadata will be not resolved. So if you don't only care about hierarchy
    /// but also the metadata, you should use [TreeHandler::get_value_with_meta()].
    pub fn get_value(&self) -> LoroValue {
        self.inner.get_value().into()
    }

    /// Return the flat array of the forest, each node is with metadata.
    pub fn get_value_with_meta(&self) -> LoroValue {
        self.inner.get_value_with_meta().into()
    }

    /// Whether the fractional index is enabled.
    pub fn is_fractional_index_enabled(&self) -> bool {
        self.inner.is_fractional_index_enabled()
    }

    /// Enable fractional index for Tree Position.
    ///
    /// The jitter is used to avoid conflicts when multiple users are creating the node at the same position.
    /// value 0 is default, which means no jitter, any value larger than 0 will enable jitter.
    ///
    /// Generally speaking, jitter will affect the growth rate of document size.
    /// [Read more about it](https://www.loro.dev/blog/movable-tree#implementation-and-encoding-size)
    #[inline]
    pub fn enable_fractional_index(&self, jitter: u8) {
        self.inner.enable_fractional_index(jitter);
    }

    /// Disable the fractional index generation for Tree Position when
    /// you don't need the Tree's siblings to be sorted. The fractional index will be always default.
    #[inline]
    pub fn disable_fractional_index(&self) {
        self.inner.disable_fractional_index();
    }

    pub fn is_deleted(&self) -> bool {
        self.inner.is_deleted()
    }

    pub fn get_last_move_id(&self, target: &TreeID) -> Option<ID> {
        self.inner.get_last_move_id(target)
    }

    pub fn doc(&self) -> Option<Arc<LoroDoc>> {
        self.inner.doc().map(|x| Arc::new(LoroDoc { doc: x }))
    }
}

impl Default for LoroTree {
    fn default() -> Self {
        Self::new()
    }
}

impl From<loro::TreeParentId> for TreeParentId {
    fn from(value: loro::TreeParentId) -> Self {
        match value {
            loro::TreeParentId::Node(id) => Self::Node { id },
            loro::TreeParentId::Root => Self::Root,
            loro::TreeParentId::Deleted => Self::Deleted,
            loro::TreeParentId::Unexist => Self::Unexist,
        }
    }
}

impl From<TreeParentId> for loro::TreeParentId {
    fn from(value: TreeParentId) -> Self {
        match value {
            TreeParentId::Node { id } => Self::Node(id),
            TreeParentId::Root => Self::Root,
            TreeParentId::Deleted => Self::Deleted,
            TreeParentId::Unexist => Self::Unexist,
        }
    }
}
