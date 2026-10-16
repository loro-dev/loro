use std::ops::Range;

use append_only_bytes::BytesSlice;

use crate::container::richtext::{richtext_state::unicode_to_utf8_index, RichtextChunk};

/// Non-ASCII op text is cut into pieces of at most this many bytes, so that
/// finding a unicode offset inside a piece scans at most this many bytes.
const MAX_PIECE_BYTES: usize = 256;

/// The text of the ops applied to a rich-text tracker.
///
/// The tracker's text chunks are unicode ranges in this table, which holds the
/// ops' own [`BytesSlice`]s. Ops decoded from a change block keep their text in
/// a buffer owned by that decode, not in the doc's string arena, so the tracker
/// cannot address them by arena position. The table is reset with its tracker.
/// See `context/text-arena-growth.md`.
#[derive(Debug, Default)]
pub(super) struct TrackerTexts {
    /// `(unicode start, text)`, sorted by start. A piece ends where the next
    /// one starts, the last one at `len`.
    pieces: Vec<(u32, BytesSlice)>,
    len: u32,
}

impl TrackerTexts {
    pub fn clear(&mut self) {
        self.pieces.clear();
        self.len = 0;
    }

    /// Records the `unicode_len` chars of `text` and returns their range.
    pub fn push(&mut self, text: &BytesSlice, unicode_len: u32) -> Range<u32> {
        let start = self.len;
        let end = start
            .checked_add(unicode_len)
            .filter(|&end| end < RichtextChunk::MOVE_ANCHOR)
            .expect("rich-text tracker text exceeds the u32 index space");
        if unicode_len == 0 {
            return start..end;
        }

        if text.len() == unicode_len as usize || text.len() <= MAX_PIECE_BYTES {
            self.pieces.push((start, text.clone()));
        } else {
            let s = as_str(text);
            let mut piece_byte = 0;
            let mut piece_unicode = start;
            let mut unicode = start;
            for (byte, _) in s.char_indices() {
                if byte - piece_byte >= MAX_PIECE_BYTES {
                    self.pieces
                        .push((piece_unicode, text.slice_clone(piece_byte..byte)));
                    piece_byte = byte;
                    piece_unicode = unicode;
                }
                unicode += 1;
            }
            assert_eq!(unicode, end, "op unicode_len does not match its text");
            self.pieces
                .push((piece_unicode, text.slice_clone(piece_byte..)));
        }

        self.len = end;
        start..end
    }

    /// Calls `f(offset, text)` for the text in `range`, in order. `offset` is
    /// the unicode offset of `text` from `range.start`. Neighboring pieces of
    /// one buffer are passed as one slice.
    pub fn for_each_slice(&self, range: Range<u32>, mut f: impl FnMut(usize, BytesSlice)) {
        assert!(range.end <= self.len, "{range:?} is out of {}", self.len);
        if range.start >= range.end {
            return;
        }

        let mut i = self
            .pieces
            .partition_point(|(start, _)| *start <= range.start)
            - 1;
        let mut pending: Option<(usize, BytesSlice)> = None;
        let mut cur = range.start;
        while cur < range.end {
            let (piece_start, piece) = &self.pieces[i];
            let piece_end = self.pieces.get(i + 1).map_or(self.len, |(start, _)| *start);
            let piece_len = (piece_end - piece_start) as usize;
            let from = (cur - piece_start) as usize;
            let to = (range.end.min(piece_end) - piece_start) as usize;
            let slice = if from == 0 && to == piece_len {
                piece.clone()
            } else if piece.len() == piece_len {
                piece.slice_clone(from..to)
            } else {
                let s = as_str(piece);
                piece.slice_clone(
                    unicode_to_utf8_index(s, from).unwrap()..unicode_to_utf8_index(s, to).unwrap(),
                )
            };

            let offset = (cur - range.start) as usize;
            match &mut pending {
                Some((_, prev)) if prev.can_merge(&slice) => prev.try_merge(&slice).unwrap(),
                _ => {
                    if let Some((offset, prev)) = pending.replace((offset, slice)) {
                        f(offset, prev);
                    }
                }
            }

            cur = piece_start + to as u32;
            i += 1;
        }

        if let Some((offset, prev)) = pending {
            f(offset, prev);
        }
    }
}

fn as_str(text: &BytesSlice) -> &str {
    // SAFETY: text op content is valid UTF-8; it was decoded or created as `str`
    unsafe { std::str::from_utf8_unchecked(text) }
}

#[cfg(test)]
mod test {
    use super::*;

    fn collect(texts: &TrackerTexts, range: Range<u32>) -> Vec<(usize, String)> {
        let mut ans = Vec::new();
        texts.for_each_slice(range, |offset, s| {
            ans.push((offset, as_str(&s).to_string()));
        });
        ans
    }

    #[test]
    fn slices_across_ops_and_pieces() {
        let mut texts = TrackerTexts::default();
        let a = BytesSlice::from_bytes("hello".as_bytes());
        let long = "文字".repeat(300);
        let b = BytesSlice::from_bytes(long.as_bytes());
        assert_eq!(texts.push(&a, 5), 0..5);
        assert_eq!(texts.push(&b, 600), 5..605);
        assert!(texts.pieces.len() > 2);

        let all = collect(&texts, 0..605);
        assert_eq!(all.len(), 2, "pieces of one buffer are passed merged");
        assert_eq!(all[0], (0, "hello".to_string()));
        assert_eq!(all[1], (5, long.clone()));

        let expected: String = long.chars().skip(95).take(300).collect();
        assert_eq!(collect(&texts, 100..400), vec![(0, expected)]);
        assert_eq!(
            collect(&texts, 3..7),
            vec![(0, "lo".to_string()), (2, "文字".to_string())]
        );
        assert!(collect(&texts, 4..4).is_empty());
    }
}
