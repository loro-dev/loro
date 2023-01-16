use rle::HasLength;

use crate::{
    container::text::text_content::SliceRange,
    id::{Counter, ID},
    span::{CounterSpan, HasId, HasIdSpan, IdSpan},
    version::IdSpanVector,
};

use super::{cursor_map::FirstCursorResult, y_span::StatusChange, Tracker};

#[derive(Debug)]
pub struct EffectIter<'a> {
    tracker: &'a mut Tracker,
    left_spans: Vec<IdSpan>,
    current_span: Option<IdSpan>,
    current_delete_targets: Option<(ID, Vec<IdSpan>)>,
}

impl<'a> EffectIter<'a> {
    pub fn new(tracker: &'a mut Tracker, target: &IdSpanVector) -> Self {
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
    Ins { pos: usize, content: SliceRange },
}

impl<'a> Iterator for EffectIter<'a> {
    type Item = Effect;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            while let Some((ref mut delete_op_id, ref mut delete_targets)) =
                self.current_delete_targets
            {
                if let Some(mut target) = delete_targets.pop() {
                    target.normalize_();
                    let result = self
                        .tracker
                        .id_to_cursor
                        .get_first_cursors_at_id_span(target)
                        .unwrap();
                    let (id, cursor) = result.into_ins().unwrap();
                    assert_eq!(id, target.id_start());
                    if cfg!(test) {
                        // SAFETY: for test
                        assert_eq!(unsafe { cursor.get_sliced() }.id, target.id_start());
                    }

                    if cursor.len != target.content_len() {
                        let new_target = IdSpan {
                            client_id: target.client_id,
                            counter: CounterSpan::new(
                                id.counter + cursor.len as Counter,
                                target.counter.end,
                            ),
                        };
                        if new_target.content_len() > 0 {
                            delete_targets.push(new_target);
                        }
                    }

                    // SAFETY: we know that the cursor is valid here
                    let pos = unsafe { cursor.get_index() };
                    let len = cursor.len;
                    *delete_op_id = delete_op_id.inc(cursor.len as Counter);
                    self.tracker.current_vv.set_end(*delete_op_id);
                    let length = -self.tracker.update_cursors(cursor, StatusChange::Delete);
                    assert!(length >= 0);
                    if length > 0 {
                        assert_eq!(length as usize, len);
                        return Some(Effect::Del { pos, len });
                    }
                } else {
                    break;
                }
            }

            if let Some(ref mut current) = self.current_span {
                let cursor = self
                    .tracker
                    .id_to_cursor
                    .get_first_cursors_at_id_span(*current);
                if let Some(cursor) = cursor {
                    match cursor {
                        FirstCursorResult::Ins(id, cursor) => {
                            debug_assert!(current.contains_id(id));
                            debug_assert!(current.contains_id(id.inc(cursor.len as Counter - 1)));
                            current
                                .counter
                                .set_start(id.counter + cursor.len as Counter);
                            // SAFETY: we know that the cursor is valid here
                            let index = unsafe { cursor.get_index() };
                            // SAFETY: cursor is valid here
                            let content = unsafe { cursor.get_sliced().slice };
                            let len = cursor.len;
                            self.tracker
                                .current_vv
                                .set_end(id.inc(cursor.len as Counter));
                            let length_diff = self
                                .tracker
                                .update_cursors(cursor, StatusChange::SetAsCurrent);

                            if length_diff > 0 {
                                debug_assert_eq!(length_diff, len as i32);
                                return Some(Effect::Ins {
                                    pos: index,
                                    content,
                                });
                            }
                        }
                        FirstCursorResult::Del(id, del) => {
                            debug_assert!(current.contains_id(id));
                            debug_assert!(
                                current.contains_id(id.inc(del.atom_len() as Counter - 1))
                            );
                            current
                                .counter
                                .set_start(id.counter + del.atom_len() as Counter);
                            self.current_delete_targets = Some((id, del.iter().cloned().collect()));
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
