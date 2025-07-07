use loro_common::LoroValue;
use loro_delta::{DeltaItem, DeltaRope};
use generic_btree::rle::HasLength;
use crate::{
    event::{Diff, ListDiff, TextDiff},
    DiffBatch,
    utils::string_slice::StringSlice,
};
use std::cmp::{min, max};

/// Enhanced transformation that properly handles all cases including deletes
pub struct EnhancedUndoTransformer;

#[derive(Debug, Clone)]
struct Range {
    start: usize,
    end: usize,
}

impl Range {
    fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
    
    fn len(&self) -> usize {
        self.end - self.start
    }
    
    fn intersects(&self, other: &Range) -> bool {
        self.start < other.end && other.start < self.end
    }
    
    fn intersection(&self, other: &Range) -> Option<Range> {
        if self.intersects(other) {
            Some(Range::new(
                max(self.start, other.start),
                min(self.end, other.end)
            ))
        } else {
            None
        }
    }
}

#[derive(Debug)]
struct PositionEffect {
    range: Range,
    effect_type: EffectType,
    shift: isize,
}

#[derive(Debug)]
enum EffectType {
    Insert,
    Delete,
}

#[derive(Debug)]
struct AdjustedDelete {
    start: usize,
    len: usize,
}

impl EnhancedUndoTransformer {
    /// Transform a DiffBatch against remote operations
    pub fn transform_diff_batch(local: &mut DiffBatch, remote: &DiffBatch) {
        for (cid, local_diff) in local.cid_to_events.iter_mut() {
            if let Some(remote_diff) = remote.cid_to_events.get(cid) {
                Self::transform_diff(local_diff, remote_diff);
            }
        }
    }
    
    /// Transform a single diff against remote operations
    fn transform_diff(local: &mut Diff, remote: &Diff) {
        match (local, remote) {
            (Diff::Text(local_text), Diff::Text(remote_text)) => {
                Self::transform_text_diff(local_text, remote_text);
            }
            (Diff::List(local_list), Diff::List(remote_list)) => {
                Self::transform_list_diff(local_list, remote_list);
            }
            (Diff::Map(local_map), Diff::Map(remote_map)) => {
                local_map.transform(remote_map, true);
            }
            (Diff::Tree(local_tree), Diff::Tree(remote_tree)) => {
                local_tree.transform(remote_tree, true);
            }
            _ => {}
        }
    }
    
    /// Transform text diff with proper handling of overlapping deletes
    fn transform_text_diff(local: &mut TextDiff, remote: &TextDiff) {
        let transformed = Self::transform_delta_items(
            local.iter().cloned().collect(),
            remote.iter().cloned().collect()
        );
        
        // Rebuild the text diff with transformed operations
        let mut new_diff = DeltaRope::new();
        for item in transformed {
            match item {
                DeltaItem::Retain { len, attr } => {
                    new_diff.push_retain(len, attr);
                }
                DeltaItem::Replace { value, attr, delete } => {
                    if value.rle_len() > 0 {
                        new_diff.push_insert(value, attr);
                    } else if delete > 0 {
                        new_diff.push_delete(delete);
                    }
                }
            }
        }
        *local = new_diff;
    }
    
    /// Transform list diff similarly
    fn transform_list_diff(local: &mut ListDiff, remote: &ListDiff) {
        // For now, use the existing transform method until we implement proper list transformation
        local.transform_(remote, true);
    }
    
    
    /// Core transformation algorithm for text operations
    fn transform_delta_items<A: Clone + Default>(
        mut local_ops: Vec<DeltaItem<StringSlice, A>>,
        mut remote_ops: Vec<DeltaItem<StringSlice, A>>
    ) -> Vec<DeltaItem<StringSlice, A>> {
        let mut result = Vec::new();
        let mut local_idx = 0;
        let mut remote_idx = 0;
        
        
        while local_idx < local_ops.len() || remote_idx < remote_ops.len() {
            if local_idx >= local_ops.len() {
                // No more local ops, we're done
                break;
            }
            
            if remote_idx >= remote_ops.len() {
                // No more remote ops, just append remaining local ops
                result.push(local_ops[local_idx].clone());
                local_idx += 1;
                continue;
            }
            
            let local_op = local_ops[local_idx].clone();
            let remote_op = remote_ops[remote_idx].clone();
            
            
            match (local_op, remote_op) {
                (DeltaItem::Retain { len: local_len, attr }, DeltaItem::Retain { len: remote_len, .. }) => {
                    let min_len = local_len.min(remote_len);
                    result.push(DeltaItem::Retain { len: min_len, attr: attr.clone() });
                    
                    if local_len > min_len {
                        local_ops[local_idx] = DeltaItem::Retain { len: local_len - min_len, attr };
                    } else {
                        local_idx += 1;
                    }
                    
                    if remote_len > min_len {
                        remote_ops[remote_idx] = DeltaItem::Retain { len: remote_len - min_len, attr: Default::default() };
                    } else {
                        remote_idx += 1;
                    }
                }
                (DeltaItem::Retain { len, attr }, DeltaItem::Replace { value, delete, .. }) => {
                    // Remote operation happens before our retain
                    if value.rle_len() > 0 {
                        // Remote insert - we need to retain over it
                        result.push(DeltaItem::Retain { len: value.rle_len(), attr: Default::default() });
                        remote_idx += 1;
                    } else if delete > 0 {
                        // Remote delete - adjust our retain length
                        let overlap = len.min(delete);
                        if len > overlap {
                            local_ops[local_idx] = DeltaItem::Retain { len: len - overlap, attr };
                        } else {
                            local_idx += 1;
                        }
                        if delete > overlap {
                            remote_ops[remote_idx] = DeltaItem::Replace { 
                                value: Default::default(), 
                                attr: Default::default(), 
                                delete: delete - overlap 
                            };
                        } else {
                            remote_idx += 1;
                        }
                    }
                }
                (DeltaItem::Replace { value, attr, delete }, DeltaItem::Retain { len: remote_len, .. }) => {
                    // Our operation happens before remote retain
                    if value.rle_len() > 0 {
                        // Our insert - just output it
                        result.push(DeltaItem::Replace { 
                            value: value.clone(), 
                            attr: attr.clone(), 
                            delete: 0 
                        });
                        local_idx += 1;
                    } else if delete > 0 {
                        // Our delete
                        let overlap = delete.min(remote_len);
                        result.push(DeltaItem::Replace {
                            value: Default::default(),
                            attr: attr.clone(),
                            delete: overlap
                        });
                        
                        if delete > overlap {
                            local_ops[local_idx] = DeltaItem::Replace {
                                value: Default::default(),
                                attr,
                                delete: delete - overlap
                            };
                        } else {
                            local_idx += 1;
                        }
                        
                        if remote_len > overlap {
                            remote_ops[remote_idx] = DeltaItem::Retain { 
                                len: remote_len - overlap, 
                                attr: Default::default() 
                            };
                        } else {
                            remote_idx += 1;
                        }
                    }
                }
                (DeltaItem::Replace { value: local_value, attr: local_attr, delete: local_delete }, 
                 DeltaItem::Replace { value: remote_value, delete: remote_delete, .. }) => {
                    // Both are operations at the same position
                    // For inserts at the same position, local goes first (left priority)
                    if local_value.rle_len() > 0 {
                        // Our insert goes first
                        result.push(DeltaItem::Replace { 
                            value: local_value.clone(), 
                            attr: local_attr.clone(), 
                            delete: 0 
                        });
                    }
                    
                    if remote_value.rle_len() > 0 {
                        // Then retain over remote insert
                        result.push(DeltaItem::Retain { len: remote_value.rle_len(), attr: Default::default() });
                    }
                    
                    // Handle deletes
                    if local_delete > 0 && remote_delete > 0 {
                        // Overlapping deletes
                        let overlap = local_delete.min(remote_delete);
                        if local_delete > overlap {
                            result.push(DeltaItem::Replace {
                                value: Default::default(),
                                attr: local_attr.clone(),
                                delete: local_delete - overlap
                            });
                        }
                    } else if local_delete > 0 {
                        result.push(DeltaItem::Replace {
                            value: Default::default(),
                            attr: local_attr.clone(),
                            delete: local_delete
                        });
                    }
                    
                    local_idx += 1;
                    remote_idx += 1;
                }
            }
        }
        
        result
    }
    
    /// Similar transformation for list operations
    fn transform_delta_items_list<A: Clone + Default>(
        local_ops: Vec<DeltaItem<Vec<LoroValue>, A>>,
        remote_ops: Vec<DeltaItem<Vec<LoroValue>, A>>
    ) -> Vec<DeltaItem<Vec<LoroValue>, A>> {
        // Similar logic but for list values
        let mut result = Vec::new();
        let mut local_pos = 0;
        
        let remote_effects = Self::calculate_position_effects_list(&remote_ops);
        
        for local_op in local_ops {
            match local_op {
                DeltaItem::Retain { len, attr } => {
                    let adjusted_len = Self::adjust_length_for_remote_changes(
                        local_pos, len, &remote_effects
                    );
                    if adjusted_len > 0 {
                        result.push(DeltaItem::Retain { len: adjusted_len, attr });
                    }
                    local_pos += len;
                }
                DeltaItem::Replace { value, attr, delete } => {
                    if value.len() > 0 {
                        // Insert operation
                        result.push(DeltaItem::Replace { value, attr: attr.clone(), delete: 0 });
                    } else if delete > 0 {
                        // Delete operation
                        let local_range = Range::new(local_pos, local_pos + delete);
                        let adjusted_delete = Self::calculate_adjusted_delete(
                            &local_range, &remote_effects
                        );
                        
                        if adjusted_delete.len > 0 {
                            result.push(DeltaItem::Replace {
                                value: Default::default(),
                                attr,
                                delete: adjusted_delete.len
                            });
                        }
                        local_pos += delete;
                    }
                }
            }
        }
        
        result
    }
    
    /// Calculate how remote operations affect positions
    fn calculate_position_effects<V, A>(ops: &[DeltaItem<V, A>]) -> Vec<PositionEffect> 
    where V: HasLength {
        let mut effects = Vec::new();
        let mut pos = 0;
        
        for op in ops {
            match op {
                DeltaItem::Retain { len, .. } => {
                    pos += len;
                }
                DeltaItem::Replace { value, delete, .. } => {
                    if value.rle_len() > 0 {
                        // Insert
                        let len = value.rle_len();
                        effects.push(PositionEffect {
                            range: Range::new(pos, pos),
                            effect_type: EffectType::Insert,
                            shift: len as isize,
                        });
                        pos += len;
                    }
                    if *delete > 0 {
                        // Delete
                        effects.push(PositionEffect {
                            range: Range::new(pos, pos + delete),
                            effect_type: EffectType::Delete,
                            shift: -(*delete as isize),
                        });
                        // Don't advance position for deletes
                    }
                }
            }
        }
        
        effects
    }
    
    fn calculate_position_effects_list<A>(ops: &[DeltaItem<Vec<LoroValue>, A>]) -> Vec<PositionEffect> {
        let mut effects = Vec::new();
        let mut pos = 0;
        
        for op in ops {
            match op {
                DeltaItem::Retain { len, .. } => {
                    pos += len;
                }
                DeltaItem::Replace { value, delete, .. } => {
                    if value.len() > 0 {
                        // Insert
                        let len = value.len();
                        effects.push(PositionEffect {
                            range: Range::new(pos, pos),
                            effect_type: EffectType::Insert,
                            shift: len as isize,
                        });
                        pos += len;
                    }
                    if *delete > 0 {
                        // Delete
                        effects.push(PositionEffect {
                            range: Range::new(pos, pos + delete),
                            effect_type: EffectType::Delete,
                            shift: -(*delete as isize),
                        });
                    }
                }
            }
        }
        
        effects
    }
    
    /// Calculate how much to shift a position based on remote changes before it
    fn calculate_position_shift(pos: usize, effects: &[PositionEffect]) -> isize {
        let mut shift = 0;
        
        for effect in effects {
            match effect.effect_type {
                EffectType::Insert => {
                    if effect.range.start <= pos {
                        shift += effect.shift;
                    }
                }
                EffectType::Delete => {
                    if effect.range.end <= pos {
                        shift += effect.shift;
                    } else if effect.range.start < pos {
                        // Partial delete before position
                        shift -= (pos - effect.range.start) as isize;
                    }
                }
            }
        }
        
        shift
    }
    
    /// Adjust a retain length based on remote deletions
    fn adjust_length_for_remote_changes(
        start_pos: usize,
        len: usize,
        effects: &[PositionEffect]
    ) -> usize {
        let range = Range::new(start_pos, start_pos + len);
        let mut adjusted_len = len;
        
        for effect in effects {
            if let EffectType::Delete = effect.effect_type {
                if let Some(intersection) = range.intersection(&effect.range) {
                    adjusted_len -= intersection.len();
                }
            }
        }
        
        adjusted_len
    }
    
    /// Calculate the adjusted delete operation after remote changes
    fn calculate_adjusted_delete(
        local_range: &Range,
        effects: &[PositionEffect]
    ) -> AdjustedDelete {
        let mut remaining_ranges = vec![local_range.clone()];
        let mut removed_ranges = Vec::new();
        
        // Process each remote effect
        for effect in effects {
            if let EffectType::Delete = effect.effect_type {
                let mut new_remaining = Vec::new();
                
                for range in &remaining_ranges {
                    if let Some(intersection) = range.intersection(&effect.range) {
                        // Part of our delete was already deleted remotely
                        removed_ranges.push(intersection);
                        
                        // Keep the parts that weren't deleted
                        if range.start < effect.range.start {
                            new_remaining.push(Range::new(range.start, effect.range.start));
                        }
                        if range.end > effect.range.end {
                            new_remaining.push(Range::new(effect.range.end, range.end));
                        }
                    } else {
                        new_remaining.push(range.clone());
                    }
                }
                
                remaining_ranges = new_remaining;
            }
        }
        
        // Calculate the total length to delete
        let total_len: usize = remaining_ranges.iter().map(|r| r.len()).sum();
        
        // Find the starting position (adjusted for remote changes)
        let start = remaining_ranges.first().map(|r| r.start).unwrap_or(local_range.start);
        
        AdjustedDelete {
            start,
            len: total_len,
        }
    }
}

#[cfg(test)]
mod tests {
    
    #[test]
    fn test_overlapping_delete_transformation() {
        // Test the specific case from the failing test
        // Local: Delete "DEF" at position 3
        // Remote: Delete "BCD" at position 1
        // Result: Local should delete only "EF"
        
        // TODO: Add specific test implementation
    }
}