use std::{char, sync::Arc};

use itertools::Itertools;
use rustc_hash::FxHashMap;

use crate::diff::{diff_impl::UpdateTimeoutError, DiffHandler};
use loro_common::{LoroError, LoroResult};

use super::TextHandler;

/// Runs one edit of a diff. [`DiffHandler`] cannot return an error, so the first
/// one is kept for [`DiffHook::finish`] and the remaining edits are skipped: they
/// were computed for a text this edit did not reach.
fn run_edit(error: &mut Option<LoroError>, edit: impl FnOnce() -> LoroResult<()>) {
    if error.is_none() {
        *error = edit().err();
    }
}

pub(super) struct DiffHook<'a> {
    text: &'a TextHandler,
    new: &'a [u32],
    last_old_index: usize,
    current_index: usize,
    error: Option<LoroError>,
}

impl<'a> DiffHook<'a> {
    pub(crate) fn new(text: &'a TextHandler, new: &'a [u32]) -> Self {
        Self {
            text,
            new,
            last_old_index: 0,
            current_index: 0,
            error: None,
        }
    }

    /// The result of `update`: an edit that failed outranks the diff's own result.
    pub(super) fn finish(
        self,
        result: Result<(), UpdateTimeoutError>,
    ) -> Result<(), UpdateTimeoutError> {
        self.error.map_or(result, |err| Err(err.into()))
    }
}

impl DiffHandler for DiffHook<'_> {
    fn insert(&mut self, old_index: usize, new_index: usize, new_len: usize) {
        if old_index > self.last_old_index {
            self.current_index += old_index - self.last_old_index;
            self.last_old_index = old_index;
        }

        let (text, new, index) = (self.text, self.new, self.current_index);
        run_edit(&mut self.error, || {
            text.insert_unicode(
                index,
                &new[new_index..new_index + new_len]
                    .iter()
                    .map(|x| char::from_u32(*x).unwrap())
                    .collect::<String>(),
            )
        });
        self.current_index += new_len;
    }

    fn delete(&mut self, old_index: usize, old_len: usize) {
        self.current_index += old_index - self.last_old_index;
        let (text, index) = (self.text, self.current_index);
        run_edit(&mut self.error, || text.delete_unicode(index, old_len));
        self.last_old_index = old_index + old_len;
    }
}

pub(super) struct DiffHookForLine<'a> {
    text: &'a TextHandler,
    old: Vec<u32>,
    new: Vec<u32>,
    lines: Vec<Arc<str>>,
    lines_lookup: FxHashMap<Arc<str>, usize>,

    last_old_index: usize,
    current_index: usize,
    error: Option<LoroError>,
}

impl<'a> DiffHookForLine<'a> {
    pub(crate) fn new(text: &'a TextHandler, new_str: &str) -> Self {
        let mut this = Self {
            text,
            old: Vec::new(),
            new: Vec::new(),
            lines: Vec::new(),
            lines_lookup: FxHashMap::default(),
            last_old_index: 0,
            current_index: 0,
            error: None,
        };

        let text_str = text.to_string();
        for line in text_str.split_inclusive('\n') {
            let line: Arc<str> = Arc::from(line);
            let id = this.register_line(line);
            this.old.push(id as u32);
        }

        for line in new_str.split_inclusive('\n') {
            let line: Arc<str> = Arc::from(line);
            let id = this.register_line(line);
            this.new.push(id as u32);
        }

        this
    }

    fn register_line(&mut self, line: Arc<str>) -> usize {
        if let Some(&index) = self.lines_lookup.get(&line) {
            return index;
        }

        self.lines.push(line.clone());
        self.lines_lookup.insert(line, self.lines.len() - 1);
        self.lines.len() - 1
    }

    pub fn get_old_arr(&self) -> &[u32] {
        &self.old
    }

    pub fn get_new_arr(&self) -> &[u32] {
        &self.new
    }

    /// See [`DiffHook::finish`].
    pub(super) fn finish(
        self,
        result: Result<(), UpdateTimeoutError>,
    ) -> Result<(), UpdateTimeoutError> {
        self.error.map_or(result, |err| Err(err.into()))
    }
}

impl DiffHandler for DiffHookForLine<'_> {
    fn insert(&mut self, old_index: usize, new_index: usize, new_len: usize) {
        if self.last_old_index < old_index {
            assert!(self.last_old_index < old_index);
            self.current_index += (self.last_old_index..old_index)
                .map(|x| self.lines[self.old[x] as usize].chars().count())
                .sum::<usize>();
            self.last_old_index = old_index;
        }

        let s = self.new[new_index..new_index + new_len]
            .iter()
            .map(|x| self.lines[*x as usize].clone())
            .join("");
        let (text, index) = (self.text, self.current_index);
        run_edit(&mut self.error, || text.insert_unicode(index, &s));
        self.current_index += s.chars().count();
    }

    fn delete(&mut self, old_index: usize, old_len: usize) {
        if self.last_old_index != old_index {
            assert!(self.last_old_index < old_index);
            self.current_index += (self.last_old_index..old_index)
                .map(|x| self.lines[self.old[x] as usize].chars().count())
                .sum::<usize>();
        }

        self.last_old_index = old_index + old_len;
        let delete_len = (old_index..old_index + old_len)
            .map(|x| self.lines[self.old[x] as usize].chars().count())
            .sum::<usize>();

        let (text, index) = (self.text, self.current_index);
        run_edit(&mut self.error, || text.delete_unicode(index, delete_len));
    }
}
