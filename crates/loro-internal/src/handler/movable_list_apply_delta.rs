use super::*;

#[derive(Debug)]
struct ReplacementContext<'a> {
    index: &'a mut usize,
    index_shift: &'a mut usize,
    to_delete: &'a mut FxHashMap<ContainerID, usize>,
    container_remap: &'a mut FxHashMap<ContainerID, ContainerID>,
    deleted_indices: &'a mut Vec<usize>,
    next_deleted: &'a mut BinaryHeap<Reverse<usize>>,
}

impl MovableListHandler {
    /// Applies a delta to the movable list handler.
    ///
    /// This function processes the given delta, performing the necessary insertions,
    /// deletions, and moves to update the list accordingly. It handles container elements,
    /// maintains a map for remapping container IDs, and ensures proper indexing throughout
    /// the operation.
    ///
    /// # Arguments
    ///
    /// * `delta` - A delta representing the changes to apply.
    /// * `container_remap` - A map used to remap container IDs during the operation.
    ///
    /// # Returns
    ///
    /// * `LoroResult<()>` - Returns `Ok(())` if successful, or an error if something goes wrong.
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn apply_delta(
        &self,
        delta: loro_delta::DeltaRope<
            loro_delta::array_vec::ArrayVec<ValueOrHandler, 8>,
            crate::event::ListDeltaMeta,
        >,
        container_remap: &mut FxHashMap<ContainerID, ContainerID>,
    ) -> LoroResult<()> {
        {
            // Test whether the delta is valid
            let len = self.len();
            let mut index = 0;
            for delta_item in delta.iter() {
                match delta_item {
                    loro_delta::DeltaItem::Retain { len, .. } => {
                        index += *len;
                    }
                    loro_delta::DeltaItem::Replace { delete, .. } => {
                        index += *delete;
                    }
                }

                if index > len {
                    return Err(LoroError::OutOfBound {
                        pos: index,
                        len,
                        info: "apply_delta".into(),
                    });
                }
            }
        }

        match &self.inner {
            MaybeDetached::Detached(_) => {
                unimplemented!();
            }
            MaybeDetached::Attached(_) => {
                // use tracing::debug;
                // debug!(
                //     "Movable list value before apply_delta: {:#?}",
                //     self.get_deep_value_with_id()
                // );
                // debug!("Applying delta: {:#?}", &delta);

                // Preprocess deletions to build a map of containers to delete.
                let mut to_delete = self.preprocess_deletions(&delta);
                // Process insertions and moves.
                let mut index = 0;
                let mut index_shift = 0;
                let mut deleted_indices = Vec::new();
                let mut next_deleted = BinaryHeap::new();

                for delta_item in delta.iter() {
                    match delta_item {
                        loro_delta::DeltaItem::Retain { len, .. } => {
                            index += len;
                        }
                        loro_delta::DeltaItem::Replace {
                            value,
                            delete,
                            attr,
                        } => {
                            // Handle deletions in the current replace operation.
                            self.handle_deletions_in_replace(
                                *delete,
                                &mut index,
                                index_shift,
                                &mut next_deleted,
                            );

                            // Process the insertions and moves.
                            let mut context = ReplacementContext {
                                index: &mut index,
                                index_shift: &mut index_shift,
                                to_delete: &mut to_delete,
                                container_remap,
                                deleted_indices: &mut deleted_indices,
                                next_deleted: &mut next_deleted,
                            };

                            self.process_replacements(value, attr, &mut context)
                                .unwrap();
                        }
                    }
                }

                // Apply any remaining deletions.
                self.apply_remaining_deletions(&delta, &mut deleted_indices)
                    .unwrap();

                Ok(())
            }
        }
    }

    /// Preprocess deletions to build a map of containers to delete.
    ///
    /// # Arguments
    ///
    /// * `delta` - The delta containing the deletions.
    ///
    /// # Returns
    ///
    /// * `FxHashMap<ContainerID, usize>` - A map of containers to their indices that need to be deleted.
    fn preprocess_deletions(
        &self,
        delta: &loro_delta::DeltaRope<
            loro_delta::array_vec::ArrayVec<ValueOrHandler, 8>,
            crate::event::ListDeltaMeta,
        >,
    ) -> FxHashMap<ContainerID, usize> {
        let mut index = 0;
        let mut to_delete = FxHashMap::default();

        for delta_item in delta.iter() {
            match delta_item {
                loro_delta::DeltaItem::Retain { len, .. } => {
                    index += len;
                }
                loro_delta::DeltaItem::Replace { delete, .. } => {
                    if *delete > 0 {
                        for i in index..index + *delete {
                            if let Some(LoroValue::Container(c)) = self.get(i) {
                                to_delete.insert(c, i);
                            }
                        }
                        index += *delete;
                    }
                }
            }
        }

        to_delete
    }

    /// Handles deletions within a replace operation.
    ///
    /// # Arguments
    ///
    /// * `delete_len` - The number of deletions.
    /// * `index` - The current index in the list.
    /// * `index_shift` - The current index shift due to previous operations.
    /// * `next_deleted` - A heap of indices scheduled for deletion.
    fn handle_deletions_in_replace(
        &self,
        delete_len: usize,
        index: &mut usize,
        index_shift: usize,
        next_deleted: &mut BinaryHeap<Reverse<usize>>,
    ) {
        if delete_len > 0 {
            let mut remaining_deletes = delete_len;
            while let Some(Reverse(old_index)) = next_deleted.peek() {
                if *old_index + index_shift < *index + remaining_deletes {
                    assert!(*index <= *old_index + index_shift);
                    assert!(remaining_deletes > 0);
                    next_deleted.pop();
                    remaining_deletes -= 1;
                } else {
                    break;
                }
            }

            // Increase the index by the number of deletions handled.
            *index += remaining_deletes;
        }
    }

    /// Processes replacements, handling insertions and moves.
    ///
    /// # Arguments
    ///
    /// * `values` - The values to insert or move.
    /// * `attr` - Additional attributes for the delta item.
    /// * `context` - A context struct containing related parameters.
    fn process_replacements(
        &self,
        values: &loro_delta::array_vec::ArrayVec<ValueOrHandler, 8>,
        attr: &crate::event::ListDeltaMeta,
        context: &mut ReplacementContext,
    ) -> LoroResult<()> {
        for v in values.iter() {
            match v {
                ValueOrHandler::Value(value) => {
                    self.insert(*context.index, value.clone())?;
                    Self::update_positions_on_insert(context.to_delete, *context.index, 1);
                    *context.index += 1;
                    *context.index_shift += 1;
                }
                ValueOrHandler::Handler(handler) => {
                    let mut old_id = handler.id();
                    if !context.to_delete.contains_key(&old_id) {
                        while let Some(new_id) = context.container_remap.get(&old_id) {
                            old_id = new_id.clone();
                            if context.to_delete.contains_key(&old_id) {
                                break;
                            }
                        }
                    }

                    if let Some(old_index) = context.to_delete.remove(&old_id) {
                        if old_index > *context.index {
                            ensure_cov::notify_cov("loro_internal::handler::movable_list_apply_delta::process_replacements::mov_0");
                            self.mov(old_index, *context.index)?;
                            context.next_deleted.push(Reverse(old_index));
                            *context.index += 1;
                            *context.index_shift += 1;
                        } else {
                            ensure_cov::notify_cov("loro_internal::handler::movable_list_apply_delta::process_replacements::mov_1");
                            self.mov(old_index, *context.index - 1)?;
                        }
                        context.deleted_indices.push(old_index);
                        Self::update_positions_on_delete(context.to_delete, old_index);
                        Self::update_positions_on_insert(context.to_delete, *context.index, 1);
                    } else if !attr.from_move {
                        // Insert a new container if not moved.
                        let new_handler = self.insert_container(
                            *context.index,
                            Handler::new_unattached(old_id.container_type()),
                        )?;
                        let new_id = new_handler.id();
                        context.container_remap.insert(old_id, new_id);
                        Self::update_positions_on_insert(context.to_delete, *context.index, 1);
                        *context.index += 1;
                        *context.index_shift += 1;
                    }
                }
            }
        }

        Ok(())
    }

    /// Applies any remaining deletions after processing insertions and moves.
    ///
    /// # Arguments
    ///
    /// * `delta` - The delta containing the deletions.
    /// * `deleted_indices` - A list of indices that have been deleted.
    fn apply_remaining_deletions(
        &self,
        delta: &loro_delta::DeltaRope<
            loro_delta::array_vec::ArrayVec<ValueOrHandler, 8>,
            crate::event::ListDeltaMeta,
        >,
        deleted_indices: &mut Vec<usize>,
    ) -> LoroResult<()> {
        // Sort deleted indices from largest to smallest.
        deleted_indices.sort_by_key(|&x| std::cmp::Reverse(x));

        let mut index = 0;
        for delta_item in delta.iter() {
            match delta_item {
                loro_delta::DeltaItem::Retain { len, .. } => {
                    index += len;
                }
                loro_delta::DeltaItem::Replace { delete, value, .. } => {
                    if *delete > 0 {
                        let mut remaining_deletes = *delete;
                        while let Some(&last) = deleted_indices.last() {
                            if last < index + remaining_deletes {
                                deleted_indices.pop();
                                remaining_deletes -= 1;
                            } else {
                                break;
                            }
                        }

                        self.delete(index, remaining_deletes)?;
                    }

                    index += value.len();
                }
            }
        }

        Ok(())
    }

    /// Updates positions in the map after an insertion.
    ///
    /// Increments positions that are greater than or equal to the insertion index.
    ///
    /// # Arguments
    ///
    /// * `positions` - The map of positions to update.
    /// * `index` - The index where the insertion occurred.
    /// * `len` - The length of the insertion.
    fn update_positions_on_insert(
        positions: &mut FxHashMap<ContainerID, usize>,
        index: usize,
        len: usize,
    ) {
        for pos in positions.values_mut() {
            if *pos >= index {
                *pos += len;
            }
        }
    }

    /// Updates positions in the map after a deletion.
    ///
    /// Decrements positions that are greater than or equal to the deletion index.
    ///
    /// # Arguments
    ///
    /// * `positions` - The map of positions to update.
    /// * `index` - The index where the deletion occurred.
    fn update_positions_on_delete(positions: &mut FxHashMap<ContainerID, usize>, index: usize) {
        for pos in positions.values_mut() {
            if *pos >= index {
                *pos -= 1;
            }
        }
    }
}
