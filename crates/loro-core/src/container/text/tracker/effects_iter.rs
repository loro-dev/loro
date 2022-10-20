use rle::HasLength;
use smallvec::smallvec;

use crate::{
    container::text::text_content::ListSlice,
    id::Counter,
    span::{CounterSpan, HasId, IdSpan},
    version::IdSpanVector,
};

use super::{cursor_map::FirstCursorResult, y_span::StatusChange, Tracker};

pub struct EffectIter<'a> {
    tracker: &'a mut Tracker,
    left_spans: Vec<IdSpan>,
    current_span: Option<IdSpan>,
    current_delete_targets: Option<Vec<IdSpan>>,
}

impl<'a> EffectIter<'a> {
    pub fn new(tracker: &'a mut Tracker, target: IdSpanVector) -> Self {
        let spans = target
            .iter()
            .map(|(client, ctr)| IdSpan::new(*client, ctr.start, ctr.end))
            .collect();

        Self {
            tracker,
            left_spans: spans,
            current_span: None,
            current_delete_targets: None,
        }
    }
}

#[derive(Debug)]
pub enum Effect {
    Del { pos: usize, len: usize },
    Ins { pos: usize, content: ListSlice },
}

impl<'a> Iterator for EffectIter<'a> {
    type Item = Effect;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(ref mut delete_targets) = self.current_delete_targets {
                let target = delete_targets.pop().unwrap();
                let result = self
                    .tracker
                    .id_to_cursor
                    .get_first_cursors_at_id_span(target)
                    .unwrap();
                let (id, cursor) = result.as_ins().unwrap();
                assert_eq!(*id, target.id_start());
                if cursor.len != target.len() {
                    delete_targets.push(IdSpan {
                        client_id: target.client_id,
                        counter: CounterSpan::new(
                            id.counter + cursor.len as Counter,
                            target.counter.end,
                        ),
                    });
                }

                // SAFETY: we know that the cursor is valid here
                let pos = unsafe { cursor.get_index() };
                let changed_len = self
                    .tracker
                    .update_cursors(smallvec![*cursor], StatusChange::Delete);
                return Some(Effect::Del {
                    pos,
                    len: (-changed_len) as usize,
                });
            }

            if let Some(ref mut current) = self.current_span {
                let cursor = self
                    .tracker
                    .id_to_cursor
                    .get_first_cursors_at_id_span(*current);
                if let Some(cursor) = cursor {
                    match cursor {
                        FirstCursorResult::Ins(id, cursor) => {
                            current
                                .counter
                                .set_start(id.counter + cursor.len as Counter);
                            // SAFETY: we know that the cursor is valid here
                            let index = unsafe { cursor.get_index() };
                            let span = IdSpan::new(
                                id.client_id,
                                id.counter,
                                id.counter + cursor.len as Counter,
                            );
                            let changed = self
                                .tracker
                                .update_cursors(smallvec![cursor], StatusChange::SetAsCurrent);
                            assert_eq!(changed as usize, span.len());
                            return Some(Effect::Ins {
                                pos: index,
                                // SAFETY: cursor is valid
                                content: unsafe { cursor.get_sliced().slice },
                            });
                        }
                        FirstCursorResult::Del(id, del) => {
                            current.counter.set_start(id.counter + del.len() as Counter);
                            self.current_delete_targets = Some(del.iter().cloned().collect());
                        }
                    }
                } else {
                    self.current_span = None;
                }
            } else {
                if self.left_spans.is_empty() {
                    return None;
                }

                self.current_span = self.left_spans.pop();
            }
        }
    }
}
