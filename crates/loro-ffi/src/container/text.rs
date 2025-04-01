use std::{fmt::Display, sync::Arc};

use loro::TextDelta as InternalTextDelta;
use loro::{cursor::Side, ContainerTrait, LoroResult, PeerID, UpdateOptions, UpdateTimeoutError};

use crate::{ContainerID, LoroDoc, LoroValue, LoroValueLike, TextDelta};

use super::Cursor;

#[derive(Debug, Clone)]
pub struct LoroText {
    pub(crate) inner: loro::LoroText,
}

impl LoroText {
    /// Create a new container that is detached from the document.
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn new() -> Self {
        Self {
            inner: loro::LoroText::new(),
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

    /// Get the [ContainerID]  of the text container.
    pub fn id(&self) -> ContainerID {
        self.inner.id().into()
    }

    /// Iterate each span(internal storage unit) of the text.
    ///
    /// The callback function will be called for each character in the text.
    /// If the callback returns `false`, the iteration will stop.
    // TODO:
    pub fn iter(&self, callback: impl FnMut(&str) -> bool) {
        self.inner.iter(callback);
    }

    /// Insert a string at the given unicode position.
    pub fn insert(&self, pos: u32, s: &str) -> LoroResult<()> {
        self.inner.insert(pos as usize, s)
    }

    /// Insert a string at the given utf-8 position.
    pub fn insert_utf8(&self, pos: u32, s: &str) -> LoroResult<()> {
        self.inner.insert_utf8(pos as usize, s)
    }

    /// Delete a range of text at the given unicode position with unicode length.
    pub fn delete(&self, pos: u32, len: u32) -> LoroResult<()> {
        self.inner.delete(pos as usize, len as usize)
    }

    /// Delete a range of text at the given utf-8 position with utf-8 length.
    pub fn delete_utf8(&self, pos: u32, len: u32) -> LoroResult<()> {
        self.inner.delete_utf8(pos as usize, len as usize)
    }

    /// Get a string slice at the given Unicode range
    pub fn slice(&self, start_index: u32, end_index: u32) -> LoroResult<String> {
        self.inner.slice(start_index as usize, end_index as usize)
    }

    /// Get the characters at given unicode position.
    // TODO:
    pub fn char_at(&self, pos: u32) -> LoroResult<char> {
        self.inner.char_at(pos as usize)
    }

    /// Delete specified character and insert string at the same position at given unicode position.
    pub fn splice(&self, pos: u32, len: u32, s: &str) -> LoroResult<String> {
        self.inner.splice(pos as usize, len as usize, s)
    }

    /// Whether the text container is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Get the length of the text container in UTF-8.
    pub fn len_utf8(&self) -> u32 {
        self.inner.len_utf8() as u32
    }

    /// Get the length of the text container in Unicode.
    pub fn len_unicode(&self) -> u32 {
        self.inner.len_unicode() as u32
    }

    /// Get the length of the text container in UTF-16.
    pub fn len_utf16(&self) -> u32 {
        self.inner.len_utf16() as u32
    }

    /// Update the current text based on the provided text.
    pub fn update(&self, text: &str, options: UpdateOptions) -> Result<(), UpdateTimeoutError> {
        self.inner.update(text, options)
    }

    /// Apply a [delta](https://quilljs.com/docs/delta/) to the text container.
    // TODO:
    pub fn apply_delta(&self, delta: Vec<TextDelta>) -> LoroResult<()> {
        let internal_delta: Vec<InternalTextDelta> = delta.into_iter().map(|d| d.into()).collect();
        self.inner.apply_delta(&internal_delta)
    }

    /// Mark a range of text with a key-value pair.
    ///
    /// You can use it to create a highlight, make a range of text bold, or add a link to a range of text.
    ///
    /// You can specify the `expand` option to set the behavior when inserting text at the boundary of the range.
    ///
    /// - `after`(default): when inserting text right after the given range, the mark will be expanded to include the inserted text
    /// - `before`: when inserting text right before the given range, the mark will be expanded to include the inserted text
    /// - `none`: the mark will not be expanded to include the inserted text at the boundaries
    /// - `both`: when inserting text either right before or right after the given range, the mark will be expanded to include the inserted text
    ///
    /// *You should make sure that a key is always associated with the same expand type.*
    ///
    /// Note: this is not suitable for unmergeable annotations like comments.
    pub fn mark(
        &self,
        from: u32,
        to: u32,
        key: &str,
        value: Arc<dyn LoroValueLike>,
    ) -> LoroResult<()> {
        self.inner
            .mark(from as usize..to as usize, key, value.as_loro_value())
    }

    /// Unmark a range of text with a key and a value.
    ///
    /// You can use it to remove highlights, bolds or links
    ///
    /// You can specify the `expand` option to set the behavior when inserting text at the boundary of the range.
    ///
    /// **Note: You should specify the same expand type as when you mark the text.**
    ///
    /// - `after`(default): when inserting text right after the given range, the mark will be expanded to include the inserted text
    /// - `before`: when inserting text right before the given range, the mark will be expanded to include the inserted text
    /// - `none`: the mark will not be expanded to include the inserted text at the boundaries
    /// - `both`: when inserting text either right before or right after the given range, the mark will be expanded to include the inserted text
    ///
    /// *You should make sure that a key is always associated with the same expand type.*
    ///
    /// Note: you cannot delete unmergeable annotations like comments by this method.
    pub fn unmark(&self, from: u32, to: u32, key: &str) -> LoroResult<()> {
        self.inner.unmark(from as usize..to as usize, key)
    }

    /// Get the text in [Delta](https://quilljs.com/docs/delta/) format.
    ///
    /// # Example
    /// ```
    /// use loro::{LoroDoc, ToJson, ExpandType, TextDelta};
    /// use serde_json::json;
    /// use std::collections::HashMap;
    ///
    /// let doc = LoroDoc::new();
    /// let text = doc.get_text("text");
    /// text.insert(0, "Hello world!").unwrap();
    /// text.mark(0..5, "bold", true).unwrap();
    /// assert_eq!(
    ///     text.to_delta(),
    ///     vec![
    ///         TextDelta::Insert {
    ///             insert: "Hello".to_string(),
    ///             attributes: Some(HashMap::from_iter([("bold".to_string(), true.into())])),
    ///         },
    ///         TextDelta::Insert {
    ///             insert: " world!".to_string(),
    ///             attributes: None,
    ///         },
    ///     ]
    /// );
    /// text.unmark(3..5, "bold").unwrap();
    /// assert_eq!(
    ///     text.to_delta(),
    ///     vec![
    ///         TextDelta::Insert {
    ///             insert: "Hel".to_string(),
    ///             attributes: Some(HashMap::from_iter([("bold".to_string(), true.into())])),
    ///         },
    ///         TextDelta::Insert {
    ///             insert: "lo world!".to_string(),
    ///             attributes: None,
    ///         },
    ///     ]
    /// );
    /// ```
    pub fn to_delta(&self) -> Vec<TextDelta> {
        self.inner
            .to_delta()
            .into_iter()
            .map(|d| d.into())
            .collect()
    }

    /// Get the text in [Delta](https://quilljs.com/docs/delta/) format.
    ///
    /// # Example
    /// ```
    /// # use loro::{LoroDoc, ToJson, ExpandType};
    /// # use serde_json::json;
    ///
    /// let doc = LoroDoc::new();
    /// let text = doc.get_text("text");
    /// text.insert(0, "Hello world!").unwrap();
    /// text.mark(0..5, "bold", true).unwrap();
    /// assert_eq!(
    ///     text.get_richtext_value().to_json_value(),
    ///     json!([
    ///         { "insert": "Hello", "attributes": {"bold": true} },
    ///         { "insert": " world!" },
    ///     ])
    /// );
    /// text.unmark(3..5, "bold").unwrap();
    /// assert_eq!(
    ///     text.get_richtext_value().to_json_value(),
    ///     json!([
    ///         { "insert": "Hel", "attributes": {"bold": true} },
    ///         { "insert": "lo world!" },
    ///    ])
    /// );
    /// ```
    pub fn get_richtext_value(&self) -> LoroValue {
        self.inner.get_richtext_value().into()
    }

    /// Get the cursor at the given position.
    ///
    /// Using "index" to denote cursor positions can be unstable, as positions may
    /// shift with document edits. To reliably represent a position or range within
    /// a document, it is more effective to leverage the unique ID of each item/character
    /// in a List CRDT or Text CRDT.
    ///
    /// Loro optimizes State metadata by not storing the IDs of deleted elements. This
    /// approach complicates tracking cursors since they rely on these IDs. The solution
    /// recalculates position by replaying relevant history to update stable positions
    /// accurately. To minimize the performance impact of history replay, the system
    /// updates cursor info to reference only the IDs of currently present elements,
    /// thereby reducing the need for replay.
    ///
    /// # Example
    ///
    /// ```
    /// # use loro::{LoroDoc, ToJson};
    /// let doc = LoroDoc::new();
    /// let text = &doc.get_text("text");
    /// text.insert(0, "01234").unwrap();
    /// let pos = text.get_cursor(5, Default::default()).unwrap();
    /// assert_eq!(doc.get_cursor_pos(&pos).unwrap().current.pos, 5);
    /// text.insert(0, "01234").unwrap();
    /// assert_eq!(doc.get_cursor_pos(&pos).unwrap().current.pos, 10);
    /// text.delete(0, 10).unwrap();
    /// assert_eq!(doc.get_cursor_pos(&pos).unwrap().current.pos, 0);
    /// text.insert(0, "01234").unwrap();
    /// assert_eq!(doc.get_cursor_pos(&pos).unwrap().current.pos, 5);
    /// ```
    pub fn get_cursor(&self, pos: u32, side: Side) -> Option<Arc<Cursor>> {
        self.inner
            .get_cursor(pos as usize, side)
            .map(|v| Arc::new(v.into()))
    }

    pub fn update_by_line(
        &self,
        text: &str,
        options: UpdateOptions,
    ) -> Result<(), UpdateTimeoutError> {
        self.inner.update_by_line(text, options)
    }

    pub fn is_deleted(&self) -> bool {
        self.inner.is_deleted()
    }

    pub fn push_str(&self, s: &str) -> LoroResult<()> {
        self.inner.push_str(s)
    }

    pub fn get_editor_at_unicode_pos(&self, pos: u32) -> Option<PeerID> {
        self.inner.get_editor_at_unicode_pos(pos as usize)
    }

    pub fn doc(&self) -> Option<Arc<LoroDoc>> {
        self.inner.doc().map(|x| Arc::new(LoroDoc { doc: x }))
    }
}

impl Display for LoroText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inner.to_string())
    }
}

impl Default for LoroText {
    fn default() -> Self {
        Self::new()
    }
}
