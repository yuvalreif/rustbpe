use super::*;

impl CompositionalTokenizer {
    pub(super) fn find_longest_boundary_safe_match(
        &self,
        raw_ids: &[u32],
        start_idx: usize,
        space_prefix_prefix_sum: &[usize],
    ) -> Option<EntryValue> {
        if start_idx >= raw_ids.len() {
            return None;
        }
        let start_inside_word = start_idx > 0
            && self.token_meta_ref(raw_ids[start_idx]).has_word_char
            && !self.token_meta_ref(raw_ids[start_idx]).has_space_prefix
            && self.token_meta_ref(raw_ids[start_idx - 1]).has_word_char;
        let mut node_idx = 0usize;
        let max_end = usize::min(start_idx + self.max_sequence_len, raw_ids.len());
        let mut best: Option<EntryValue> = None;
        let mut best_length = 0usize;
        for end_idx in start_idx..max_end {
            let token_id = raw_ids[end_idx];
            let child = match self.trie_nodes[node_idx].children.get(&token_id) {
                Some(idx) => *idx,
                None => break,
            };
            node_idx = child;
            let Some(entry) = self.trie_nodes[node_idx].value.clone() else {
                continue;
            };
            let span_end = end_idx + 1;
            let match_length = span_end - start_idx;
            let combined_modifier = self.combine_modifier_rows(&entry.modifier_rows);
            let allow_intra_word_cap_alias =
                self.is_intra_word_cap_alias_match(match_length, &combined_modifier);
            if start_inside_word && !allow_intra_word_cap_alias {
                continue;
            }
            if span_end < raw_ids.len()
                && self.token_meta_ref(raw_ids[end_idx]).has_word_char
                && !self.token_meta_ref(raw_ids[span_end]).has_space_prefix
                && self.token_meta_ref(raw_ids[span_end]).has_word_char
                && !allow_intra_word_cap_alias
            {
                continue;
            }
            if match_length > 1 {
                let all_word =
                    (start_idx..span_end).all(|j| self.token_meta_ref(raw_ids[j]).has_word_char);
                if all_word
                    && (space_prefix_prefix_sum[span_end] - space_prefix_prefix_sum[start_idx + 1])
                        > 0
                {
                    continue;
                }
            }
            best = Some(entry);
            best_length = match_length;
        }
        if best_length > 0 {
            best
        } else {
            None
        }
    }

    pub(super) fn should_prefer_cap_fallback_over_match(
        &self,
        raw_ids: &[u32],
        start_idx: usize,
        entry: &EntryValue,
    ) -> bool {
        let match_length = entry.consumed_len;
        let modifier = self.combine_modifier_rows(&entry.modifier_rows);
        if !self.is_intra_word_cap_alias_match(match_length, &modifier) {
            return false;
        }
        if start_idx >= raw_ids.len() || !self.token_meta_ref(raw_ids[start_idx]).has_word_char {
            return false;
        }
        let prev_continues_word = start_idx > 0
            && self.token_meta_ref(raw_ids[start_idx - 1]).has_word_char
            && !self.token_meta_ref(raw_ids[start_idx]).has_space_prefix;
        let next_idx = start_idx + match_length;
        let next_continues_word = next_idx < raw_ids.len()
            && self.token_meta_ref(raw_ids[next_idx]).has_word_char
            && !self.token_meta_ref(raw_ids[next_idx]).has_space_prefix;
        prev_continues_word || next_continues_word
    }

    pub(super) fn try_lowercase_cap_fallback(
        &self,
        raw_ids: &[u32],
        start_idx: usize,
        pending_groups: &[PendingGroup],
        pending_leading_space: bool,
    ) -> Option<(usize, Vec<u32>, Vec<Vec<u16>>)> {
        let base_cap_idx = self.group_idx("base_capitalization")?;
        if start_idx >= raw_ids.len() || !self.token_meta_ref(raw_ids[start_idx]).has_word_char {
            return None;
        }
        let mut end_idx = start_idx + 1;
        while end_idx < raw_ids.len() {
            let meta = self.token_meta_ref(raw_ids[end_idx]);
            if meta.has_space_prefix || !meta.has_word_char || meta.is_whitespace_only {
                break;
            }
            end_idx += 1;
        }
        let mut surface = self.decode_ids(&raw_ids[start_idx..end_idx]);
        surface = surface.trim().to_string();
        if surface.is_empty()
            || !surface.chars().all(|ch| ch.is_alphabetic())
            || !surface.chars().any(|ch| ch.is_uppercase())
        {
            return None;
        }
        if !is_base_cap_representable_surface(&surface) {
            return None;
        }
        let split_segments = split_camel_case_segments(&surface);
        let is_title_surface = surface
            .chars()
            .next()
            .map(|ch| ch.is_uppercase())
            .unwrap_or(false)
            && surface
                .chars()
                .skip(1)
                .all(|ch| !ch.is_alphabetic() || ch.is_lowercase());
        if split_segments.is_none() && !is_title_surface {
            return None;
        }
        let segments = expand_caps_segments(split_segments.unwrap_or_else(|| vec![surface]));
        let mut output_ids = Vec::new();
        let mut output_mods = Vec::new();
        let mut first_output = true;
        let space_idx = self.group_idx("space_prefix");
        for segment in segments {
            let lower_ids = self.encode_segment(&segment.to_lowercase())?;
            if lower_ids.is_empty() {
                return None;
            }
            let mut base_modifier = self.empty_modifier();
            base_modifier[base_cap_idx] = 1;
            if first_output {
                if pending_leading_space {
                    if let Some(idx) = space_idx {
                        base_modifier[idx] = 1;
                    }
                }
                base_modifier = self.combine_pending(&base_modifier, pending_groups);
            }
            let per_token_mods = self.spread_multi_token_modifiers(&base_modifier, lower_ids.len());
            output_ids.extend(lower_ids);
            output_mods.extend(per_token_mods);
            first_output = false;
        }
        Some((end_idx - start_idx, output_ids, output_mods))
    }

    pub(super) fn can_attach_detached_modifier(
        &self,
        raw_ids: &[u32],
        start_idx: usize,
        consumed_len: usize,
        pending_has_prefix_punct: bool,
    ) -> bool {
        if consumed_len == 0 || start_idx >= raw_ids.len() {
            return false;
        }
        let span_end = usize::min(start_idx + consumed_len, raw_ids.len());
        let left_ok = if start_idx == 0 {
            true
        } else {
            self.token_meta_ref(raw_ids[start_idx]).has_space_prefix
                || self
                    .token_meta_ref(raw_ids[start_idx - 1])
                    .is_whitespace_only
        };
        if !left_ok && !pending_has_prefix_punct {
            return false;
        }
        let mut j = span_end;
        let mut saw_whitespace_between = false;
        while j < raw_ids.len() && self.token_meta_ref(raw_ids[j]).is_whitespace_only {
            if !self.token_meta_ref(raw_ids[j]).is_single_ascii_space {
                return false;
            }
            saw_whitespace_between = true;
            j += 1;
        }
        if j >= raw_ids.len() || !self.raw_position_has_word_char(raw_ids, j) {
            return false;
        }
        let next_surface = &self.token_meta_ref(raw_ids[j]).canonical_surface;
        let current_surface = &self.token_meta_ref(raw_ids[start_idx]).canonical_surface;
        let has_prep = self
            .runtime
            .literal_maps
            .get("prepositions")
            .map(|m| m.contains_key(next_surface))
            .unwrap_or(false);
        if has_prep {
            return false;
        }
        let current_is_prep = self
            .runtime
            .literal_maps
            .get("prepositions")
            .map(|m| m.contains_key(current_surface))
            .unwrap_or(false);
        let next_is_det = self
            .runtime
            .literal_maps
            .get("determiners")
            .map(|m| m.contains_key(next_surface))
            .unwrap_or(false);
        if current_is_prep && next_is_det {
            return true;
        }
        if next_is_det {
            return false;
        }
        saw_whitespace_between || self.token_meta_ref(raw_ids[j]).has_space_prefix
    }
}
