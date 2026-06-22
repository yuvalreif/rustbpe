use super::*;

impl CompositionalTokenizer {
    pub(super) fn process_ids_impl(&self, raw_ids: &[u32]) -> (Vec<u32>, Vec<Vec<u16>>) {
        let mut out_ids = Vec::new();
        let mut out_mods = Vec::new();
        let space_prefix_prefix_sum: Vec<usize> = {
            let mut out = vec![0usize; raw_ids.len() + 1];
            let mut running = 0usize;
            for (idx, token_id) in raw_ids.iter().enumerate() {
                if self.token_meta_ref(*token_id).has_space_prefix {
                    running += 1;
                }
                out[idx + 1] = running;
            }
            out
        };

        let article_cap_idx = self.group_idx("article_capitalization");
        let prep_cap_idx = self.group_idx("prep_capitalization");
        let space_idx = self.group_idx("space_prefix");
        let suffix_group_name = self
            .group_idx("suffix_punctuation")
            .map(|_| "suffix_punctuation".to_string());

        let mut pending_groups: Vec<PendingGroup> = Vec::new();
        let mut pending_token_records: Vec<(usize, u32)> = Vec::new();
        let mut pending_leading_space = false;

        let literal_modifier =
            |this: &Self, start_idx: usize, token_id: u32, force_leading_space: bool| -> Vec<u16> {
                let mut modifier = this.empty_modifier();
                if let Some(idx) = space_idx {
                    if !this.token_meta_ref(token_id).is_whitespace_only
                        && (force_leading_space
                            || this.raw_expr_has_leading_space(raw_ids, start_idx))
                    {
                        modifier[idx] = 1;
                    }
                }
                modifier
            };

        let emit_literal = |this: &Self,
                            start_idx: usize,
                            token_id: u32,
                            force_leading_space: bool,
                            out_ids: &mut Vec<u32>,
                            out_mods: &mut Vec<Vec<u16>>| {
            out_ids.push(token_id);
            out_mods.push(literal_modifier(
                this,
                start_idx,
                token_id,
                force_leading_space,
            ));
        };

        let flush_pending_literal =
            |this: &Self,
             pending_leading_space_ref: &mut bool,
             pending_groups_ref: &mut Vec<PendingGroup>,
             pending_token_records_ref: &mut Vec<(usize, u32)>,
             out_ids_ref: &mut Vec<u32>,
             out_mods_ref: &mut Vec<Vec<u16>>| {
                let mut emit_leading_space = *pending_leading_space_ref;
                for (raw_idx, raw_token_id) in pending_token_records_ref.iter().copied() {
                    let is_whitespace_only = this.token_meta_ref(raw_token_id).is_whitespace_only;
                    emit_literal(
                        this,
                        raw_idx,
                        raw_token_id,
                        emit_leading_space && !is_whitespace_only,
                        out_ids_ref,
                        out_mods_ref,
                    );
                    if emit_leading_space {
                        emit_leading_space = false;
                    }
                }
                pending_groups_ref.clear();
                pending_token_records_ref.clear();
                *pending_leading_space_ref = false;
            };

        let mark_pending_detached_prefix =
            |this: &Self, start_idx: usize, pending_leading_space_ref: &mut bool| {
                if !*pending_leading_space_ref
                    && this.raw_expr_has_leading_space(raw_ids, start_idx)
                {
                    *pending_leading_space_ref = true;
                }
            };

        let mut idx = 0usize;
        while idx < raw_ids.len() {
            let token_id = raw_ids[idx];
            let meta = self.token_meta_ref(token_id).clone();

            if meta.is_whitespace_only {
                if meta.token_text == " " {
                    if !pending_groups.is_empty() {
                        pending_token_records.push((idx, token_id));
                    } else if self.token_can_host_expr_space(raw_ids, idx + 1) {
                        pending_leading_space = true;
                    } else {
                        emit_literal(self, idx, token_id, false, &mut out_ids, &mut out_mods);
                    }
                    idx += 1;
                    continue;
                }
                if !pending_groups.is_empty() || !pending_token_records.is_empty() {
                    flush_pending_literal(
                        self,
                        &mut pending_leading_space,
                        &mut pending_groups,
                        &mut pending_token_records,
                        &mut out_ids,
                        &mut out_mods,
                    );
                }
                emit_literal(self, idx, token_id, false, &mut out_ids, &mut out_mods);
                idx += 1;
                continue;
            }

            if meta.is_byte_fallback {
                let mut first_modifier =
                    if !pending_groups.is_empty() || !pending_token_records.is_empty() {
                        self.combine_pending(&self.empty_modifier(), &pending_groups)
                    } else {
                        self.empty_modifier()
                    };
                if let Some(space_group_idx) = space_idx {
                    if pending_leading_space {
                        first_modifier[space_group_idx] = 1;
                    }
                }
                let component_end = self.byte_component_end(raw_ids, idx);
                for (component_idx, token_id) in
                    raw_ids.iter().enumerate().take(component_end).skip(idx)
                {
                    out_ids.push(*token_id);
                    if component_idx == idx {
                        out_mods.push(first_modifier.clone());
                    } else {
                        out_mods.push(self.empty_modifier());
                    }
                }
                pending_groups.clear();
                pending_token_records.clear();
                pending_leading_space = false;
                idx = component_end;
                continue;
            }

            if let Some(suffix_transform) = meta.suffix_punctuation.clone() {
                if self.group_idx("suffix_punctuation").is_some() && !out_mods.is_empty() {
                    let prev_is_whitespace =
                        idx == 0 || self.token_meta_ref(raw_ids[idx - 1]).is_whitespace_only;
                    let already_has_suffix = suffix_group_name
                        .as_ref()
                        .map(|name| self.modifier_has_active_group(out_mods.last().unwrap(), name))
                        .unwrap_or(false);
                    if !prev_is_whitespace
                        && !meta.has_space_prefix
                        && !already_has_suffix
                        && self.runtime.attachment_limits.max_suffix_punctuation > 0
                    {
                        if let Some(group_idx) = self.group_idx(&suffix_transform.group_name) {
                            out_mods.last_mut().unwrap()[group_idx] =
                                suffix_transform.rel_idx as u16;
                            idx += 1;
                            continue;
                        }
                    }
                }
            }

            if let Some(prefix_transform) = meta.prefix_punctuation.clone() {
                let next_idx = idx + 1;
                if self.runtime.attachment_limits.max_prefix_punctuation > 0
                    && next_idx < raw_ids.len()
                    && !self.token_meta_ref(raw_ids[next_idx]).is_whitespace_only
                    && !self.token_meta_ref(raw_ids[next_idx]).has_space_prefix
                    && self.raw_position_has_word_char(raw_ids, next_idx)
                {
                    if let Some(group_idx) = self.group_idx(&prefix_transform.group_name) {
                        pending_groups.push(PendingGroup {
                            group_idx,
                            rel_idx: prefix_transform.rel_idx as u16,
                        });
                        pending_token_records.push((idx, token_id));
                        idx += 1;
                        continue;
                    }
                }
                if !pending_groups.is_empty() || !pending_token_records.is_empty() {
                    flush_pending_literal(
                        self,
                        &mut pending_leading_space,
                        &mut pending_groups,
                        &mut pending_token_records,
                        &mut out_ids,
                        &mut out_mods,
                    );
                }
            }

            if let Some(det_transform) = meta.determiner.clone() {
                if meta.is_base_cap_representable
                    && self.can_attach_detached_modifier(
                        raw_ids,
                        idx,
                        1,
                        !pending_groups.is_empty(),
                    )
                {
                    mark_pending_detached_prefix(self, idx, &mut pending_leading_space);
                    if let Some(group_idx) = self.group_idx(&det_transform.group_name) {
                        pending_groups.push(PendingGroup {
                            group_idx,
                            rel_idx: det_transform.rel_idx as u16,
                        });
                        pending_token_records.push((idx, token_id));
                        if is_capitalized_surface(&meta.token_text)
                            && meta.is_base_cap_representable
                        {
                            if let Some(cap_idx) = article_cap_idx {
                                pending_groups.push(PendingGroup {
                                    group_idx: cap_idx,
                                    rel_idx: 1,
                                });
                            }
                        }
                        idx += 1;
                        continue;
                    }
                }
            }

            if let Some(prep_transform) = meta.preposition.clone() {
                if meta.is_base_cap_representable
                    && self.can_attach_detached_modifier(
                        raw_ids,
                        idx,
                        1,
                        !pending_groups.is_empty(),
                    )
                {
                    mark_pending_detached_prefix(self, idx, &mut pending_leading_space);
                    if let Some(group_idx) = self.group_idx(&prep_transform.group_name) {
                        pending_groups.push(PendingGroup {
                            group_idx,
                            rel_idx: prep_transform.rel_idx as u16,
                        });
                        pending_token_records.push((idx, token_id));
                        if is_capitalized_surface(&meta.token_text)
                            && meta.is_base_cap_representable
                        {
                            if let Some(cap_idx) = prep_cap_idx {
                                pending_groups.push(PendingGroup {
                                    group_idx: cap_idx,
                                    rel_idx: 1,
                                });
                            }
                        }
                        idx += 1;
                        continue;
                    }
                }
            }

            if !pending_groups.is_empty()
                && (meta.determiner.is_some() || meta.preposition.is_some())
            {
                flush_pending_literal(
                    self,
                    &mut pending_leading_space,
                    &mut pending_groups,
                    &mut pending_token_records,
                    &mut out_ids,
                    &mut out_mods,
                );
                continue;
            }

            let mut entry =
                self.find_longest_boundary_safe_match(raw_ids, idx, &space_prefix_prefix_sum);
            if let Some(found) = entry.clone() {
                if self.should_prefer_cap_fallback_over_match(raw_ids, idx, &found) {
                    entry = None;
                } else {
                    let mut combined_modifier = self.combine_modifier_rows(&found.modifier_rows);
                    let base_surface = self
                        .decode_ids(&found.base_ids)
                        .trim_start_matches(' ')
                        .to_lowercase();
                    let raw_surface = self.decode_ids(&raw_ids[idx..idx + found.consumed_len]);

                    if found.base_ids.len() == 1
                        && self.modifier_has_only_surface_groups(&combined_modifier)
                        && self
                            .runtime
                            .literal_maps
                            .get("determiners")
                            .map(|m| m.contains_key(&base_surface))
                            .unwrap_or(false)
                        && is_base_cap_representable_surface(&raw_surface)
                        && self.can_attach_detached_modifier(
                            raw_ids,
                            idx,
                            found.consumed_len,
                            !pending_groups.is_empty(),
                        )
                    {
                        mark_pending_detached_prefix(self, idx, &mut pending_leading_space);
                        if let Some(transform) = self
                            .runtime
                            .literal_maps
                            .get("determiners")
                            .and_then(|m| m.get(&base_surface))
                        {
                            if let Some(group_idx) = self.group_idx(&transform.group_name) {
                                pending_groups.push(PendingGroup {
                                    group_idx,
                                    rel_idx: transform.rel_idx as u16,
                                });
                                for offset in 0..found.consumed_len {
                                    pending_token_records
                                        .push((idx + offset, raw_ids[idx + offset]));
                                }
                                if is_capitalized_surface(&raw_surface)
                                    && is_base_cap_representable_surface(&raw_surface)
                                {
                                    if let Some(cap_idx) = article_cap_idx {
                                        pending_groups.push(PendingGroup {
                                            group_idx: cap_idx,
                                            rel_idx: 1,
                                        });
                                    }
                                }
                                idx += found.consumed_len;
                                continue;
                            }
                        }
                    }

                    if found.base_ids.len() == 1
                        && self.modifier_has_only_surface_groups(&combined_modifier)
                        && self
                            .runtime
                            .literal_maps
                            .get("prepositions")
                            .map(|m| m.contains_key(&base_surface))
                            .unwrap_or(false)
                        && is_base_cap_representable_surface(&raw_surface)
                        && self.can_attach_detached_modifier(
                            raw_ids,
                            idx,
                            found.consumed_len,
                            !pending_groups.is_empty(),
                        )
                    {
                        mark_pending_detached_prefix(self, idx, &mut pending_leading_space);
                        if let Some(transform) = self
                            .runtime
                            .literal_maps
                            .get("prepositions")
                            .and_then(|m| m.get(&base_surface))
                        {
                            if let Some(group_idx) = self.group_idx(&transform.group_name) {
                                pending_groups.push(PendingGroup {
                                    group_idx,
                                    rel_idx: transform.rel_idx as u16,
                                });
                                for offset in 0..found.consumed_len {
                                    pending_token_records
                                        .push((idx + offset, raw_ids[idx + offset]));
                                }
                                if is_capitalized_surface(&raw_surface)
                                    && is_base_cap_representable_surface(&raw_surface)
                                {
                                    if let Some(cap_idx) = prep_cap_idx {
                                        pending_groups.push(PendingGroup {
                                            group_idx: cap_idx,
                                            rel_idx: 1,
                                        });
                                    }
                                }
                                idx += found.consumed_len;
                                continue;
                            }
                        }
                    }

                    combined_modifier = self.strip_invalid_detached_modifier_groups(
                        &combined_modifier,
                        raw_ids,
                        idx,
                        found.consumed_len,
                        !pending_groups.is_empty(),
                    );
                    combined_modifier = self.apply_contextual_base_cap(
                        &combined_modifier,
                        raw_ids,
                        idx,
                        found.consumed_len,
                    );
                    combined_modifier = self.apply_contextual_space_prefix(
                        &combined_modifier,
                        raw_ids,
                        idx,
                        !pending_groups.is_empty() || pending_leading_space,
                        pending_leading_space,
                    );
                    if self.entry_has_case_mismatch(
                        &found,
                        &combined_modifier,
                        raw_ids,
                        idx,
                        found.consumed_len,
                    ) {
                        entry = None;
                    } else {
                        let combined_modifier = self
                            .strip_nonlexical_surface_groups(&combined_modifier, &found.base_ids);
                        if !pending_groups.is_empty() || pending_leading_space {
                            if found.base_ids.len() == 1 {
                                let mut merged = found.modifier_rows[0].clone();
                                merged = self.strip_invalid_detached_modifier_groups(
                                    &merged,
                                    raw_ids,
                                    idx,
                                    found.consumed_len,
                                    !pending_groups.is_empty(),
                                );
                                merged = self.apply_contextual_base_cap(
                                    &merged,
                                    raw_ids,
                                    idx,
                                    found.consumed_len,
                                );
                                merged = self.apply_contextual_space_prefix(
                                    &merged,
                                    raw_ids,
                                    idx,
                                    !pending_groups.is_empty() || pending_leading_space,
                                    pending_leading_space,
                                );
                                merged =
                                    self.strip_nonlexical_surface_groups(&merged, &found.base_ids);
                                merged = self.combine_pending(&merged, &pending_groups);
                                out_ids.extend(found.base_ids.iter().copied());
                                out_mods.push(merged);
                            } else {
                                let mut combined = combined_modifier.clone();
                                if let Some(space_group_idx) = space_idx {
                                    if pending_leading_space {
                                        combined[space_group_idx] = 1;
                                    }
                                }
                                combined = self.combine_pending(&combined, &pending_groups);
                                out_ids.extend(found.base_ids.iter().copied());
                                out_mods.extend(
                                    self.spread_multi_token_modifiers(
                                        &combined,
                                        found.base_ids.len(),
                                    ),
                                );
                            }
                        } else {
                            out_ids.extend(found.base_ids.iter().copied());
                            if found.base_ids.len() == 1 {
                                out_mods.push(combined_modifier);
                            } else {
                                let mut normalized_rows = found.modifier_rows.clone();
                                if !normalized_rows.is_empty() {
                                    normalized_rows[0] = self.apply_contextual_base_cap(
                                        &normalized_rows[0],
                                        raw_ids,
                                        idx,
                                        found.consumed_len,
                                    );
                                    normalized_rows[0] = self.apply_contextual_space_prefix(
                                        &normalized_rows[0],
                                        raw_ids,
                                        idx,
                                        pending_leading_space,
                                        pending_leading_space,
                                    );
                                    if let Some(space_group_idx) = space_idx {
                                        for row in normalized_rows.iter_mut().skip(1) {
                                            row[space_group_idx] =
                                                self.default_modifier[space_group_idx];
                                        }
                                    }
                                }
                                out_mods.extend(normalized_rows);
                            }
                        }
                        pending_groups.clear();
                        pending_token_records.clear();
                        pending_leading_space = false;
                        idx += found.consumed_len;
                        continue;
                    }
                }
            }

            if meta.has_word_char {
                if let Some((consumed_len, fallback_ids, mut fallback_mods)) = self
                    .try_lowercase_cap_fallback(
                        raw_ids,
                        idx,
                        &pending_groups,
                        pending_leading_space,
                    )
                {
                    let use_pending_space = if !pending_groups.is_empty() {
                        true
                    } else {
                        pending_leading_space
                    };
                    fallback_mods[0] = self.apply_contextual_space_prefix(
                        &fallback_mods[0],
                        raw_ids,
                        idx,
                        use_pending_space,
                        pending_leading_space,
                    );
                    out_ids.extend(fallback_ids);
                    out_mods.extend(fallback_mods);
                    pending_groups.clear();
                    pending_token_records.clear();
                    pending_leading_space = false;
                    idx += consumed_len;
                    continue;
                }
            }

            let mut base_modifier = self.empty_modifier();
            if let Some(space_group_idx) = space_idx {
                if pending_leading_space && self.raw_position_has_word_char(raw_ids, idx) {
                    base_modifier[space_group_idx] = 1;
                }
            }
            base_modifier = self.apply_contextual_base_cap(&base_modifier, raw_ids, idx, 1);
            base_modifier = self.apply_contextual_space_prefix(
                &base_modifier,
                raw_ids,
                idx,
                pending_leading_space,
                pending_leading_space,
            );
            base_modifier = self.combine_pending(&base_modifier, &pending_groups);
            out_ids.push(token_id);
            out_mods.push(base_modifier);
            pending_groups.clear();
            pending_token_records.clear();
            pending_leading_space = false;
            idx += 1;
        }

        if !pending_groups.is_empty() || !pending_token_records.is_empty() {
            flush_pending_literal(
                self,
                &mut pending_leading_space,
                &mut pending_groups,
                &mut pending_token_records,
                &mut out_ids,
                &mut out_mods,
            );
        }
        (out_ids, out_mods)
    }
}
