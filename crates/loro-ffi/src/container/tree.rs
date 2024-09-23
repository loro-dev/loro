use std::sync::Arc;

use loro::{LoroError, LoroResult, LoroTreeError, TreeID};

use crate::{ContainerID, LoroValue};

use super::LoroMap;

pub enum TreeParentId {
    Node { id: TreeID },
    Root,
    Deleted,
    Unexist,
}

#[derive(Debug, Clone)]
pub struct LoroTree {
    pub(crate) tree: loro::LoroTree,
}

impl LoroTree {
    pub fn new() -> Self {
        Self {
            tree: loro::LoroTree::new(),
        }
    }

    /// Whether the container is attached to a document
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn is_attached(&self) -> bool {
        self.tree.is_attached()
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
        self.tree.create(parent)
    }

    /// Create a new tree node at the given index and return the [`TreeID`].
    ///
    /// If the `parent` is `None`, the created node is the root of a tree.
    /// If the `index` is greater than the number of children of the parent, error will be returned.
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
    /// // create a new child at index 0
    /// let child = tree.create_at(root, 0).unwrap();
    /// ```
    pub fn create_at(&self, parent: TreeParentId, index: u32) -> LoroResult<TreeID> {
        self.tree.create_at(parent, index as usize)
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
        self.tree.mov(target, parent)
    }

    /// Move the `target` node to be a child of the `parent` node at the given index.
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
    /// // move `root2` to be a child of `root` at index 0.
    /// tree.mov_to(root2, root, 0).unwrap();
    /// ```
    pub fn mov_to(&self, target: TreeID, parent: TreeParentId, to: u32) -> LoroResult<()> {
        self.tree.mov_to(target, parent, to as usize)
    }

    /// Move the `target` node to be a child after the `after` node with the same parent.
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
    /// // move `root` to be a child after `root2`.
    /// tree.mov_after(root, root2).unwrap();
    /// ```
    pub fn mov_after(&self, target: TreeID, after: TreeID) -> LoroResult<()> {
        self.tree.mov_after(target, after)
    }

    /// Move the `target` node to be a child before the `before` node with the same parent.
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
    /// // move `root` to be a child before `root2`.
    /// tree.mov_before(root, root2).unwrap();
    /// ```
    pub fn mov_before(&self, target: TreeID, before: TreeID) -> LoroResult<()> {
        self.tree.mov_before(target, before)
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
        self.tree.delete(target)
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
        self.tree
            .get_meta(target)
            .map(|h| Arc::new(LoroMap { map: h }))
    }

    /// Return the parent of target node.
    ///
    /// - If the target node does not exist, throws Error.
    /// - If the target node is a root node, return `None`.
    pub fn parent(&self, target: TreeID) -> LoroResult<TreeParentId> {
        if let Some(p) = self.tree.parent(target) {
            Ok(p.into())
        } else {
            Err(LoroError::TreeError(LoroTreeError::TreeNodeNotExist(
                target,
            )))
        }
    }

    /// Return whether target node exists.
    pub fn contains(&self, target: TreeID) -> bool {
        self.tree.contains(target)
    }

    /// Return whether target node is deleted.
    ///
    /// # Errors
    ///
    /// - If the target node does not exist, return `LoroTreeError::TreeNodeNotExist`.
    pub fn is_node_deleted(&self, target: TreeID) -> LoroResult<bool> {
        self.tree.is_node_deleted(&target)
    }

    /// Return all nodes
    pub fn nodes(&self) -> Vec<TreeID> {
        self.tree.nodes()
    }

    /// Return all children of the target node.
    ///
    /// If the parent node does not exist, return `None`.
    pub fn children(&self, parent: TreeParentId) -> Option<Vec<TreeID>> {
        self.tree.children(parent)
    }

    /// Return the number of children of the target node.
    pub fn children_num(&self, parent: TreeParentId) -> Option<u32> {
        self.tree.children_num(parent).map(|v| v as u32)
    }

    /// Return container id of the tree.
    pub fn id(&self) -> ContainerID {
        self.tree.id().into()
    }

    /// Return the fractional index of the target node with hex format.
    pub fn fractional_index(&self, target: TreeID) -> Option<String> {
        self.tree.fractional_index(target)
    }

    /// Return the flat array of the forest.
    ///
    /// Note: the metadata will be not resolved. So if you don't only care about hierarchy
    /// but also the metadata, you should use [TreeHandler::get_value_with_meta()].
    pub fn get_value(&self) -> LoroValue {
        self.tree.get_value().into()
    }

    /// Return the flat array of the forest, each node is with metadata.
    pub fn get_value_with_meta(&self) -> LoroValue {
        self.tree.get_value_with_meta().into()
    }

    /// Whether the fractional index is enabled.
    pub fn is_fractional_index_enabled(&self) -> bool {
        self.tree.is_fractional_index_enabled()
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
        self.tree.enable_fractional_index(jitter);
    }

    /// Disable the fractional index generation for Tree Position when
    /// you don't need the Tree's siblings to be sorted. The fractional index will be always default.
    #[inline]
    pub fn disable_fractional_index(&self) {
        self.tree.disable_fractional_index();
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
            loro::TreeParentId::Node(id) => TreeParentId::Node { id },
            loro::TreeParentId::Root => TreeParentId::Root,
            loro::TreeParentId::Deleted => TreeParentId::Deleted,
            loro::TreeParentId::Unexist => TreeParentId::Unexist,
        }
    }
}

impl From<TreeParentId> for loro::TreeParentId {
    fn from(value: TreeParentId) -> Self {
        match value {
            TreeParentId::Node { id } => loro::TreeParentId::Node(id),
            TreeParentId::Root => loro::TreeParentId::Root,
            TreeParentId::Deleted => loro::TreeParentId::Deleted,
            TreeParentId::Unexist => loro::TreeParentId::Unexist,
        }
    }
}
