use std::{char, sync::Arc};

use rustc_hash::FxHashMap;
use itertools::Itertools;

use crate::diff::DiffHandler;

use super::TextHandler;

pub(super) struct DiffHook<'a> {
    text: &'a TextHandler,
    new: &'a [u32],
    last_old_index: usize,
    current_index: usize,
}

impl<'a> DiffHook<'a> {
    pub(crate) fn new(text: &'a TextHandler, new: &'a [u32]) -> Self {
        Self {
            text,
            new,
            last_old_index: 0,
            current_index: 0,
        }
    }
}

impl DiffHandler for DiffHook<'_> {
    fn insert(&mut self, old_index: usize, new_index: usize, new_len: usize) {
        if old_index > self.last_old_index {
            self.current_index += old_index - self.last_old_index;
            self.last_old_index = old_index;
        }

        self.text
            .insert_unicode(
                self.current_index,
                &self.new[new_index..new_index + new_len]
                    .iter()
                    .map(|x| char::from_u32(*x).unwrap())
                    .collect::<String>(),
            )
            .unwrap();
        self.current_index += new_len;
    }

    fn delete(&mut self, old_index: usize, old_len: usize) {
        self.current_index += old_index - self.last_old_index;
        self.text
            .delete_unicode(self.current_index, old_len)
            .unwrap();
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
        self.text.insert_unicode(self.current_index, &s).unwrap();
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

        self.text
            .delete_unicode(self.current_index, delete_len)
            .unwrap();
    }
}
