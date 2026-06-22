import json

import rustbpe


def _config():
    group_names = [
        "space_prefix",
        "base_capitalization",
        "determiners",
        "article_capitalization",
        "prepositions",
        "prep_capitalization",
        "prefix_punctuation",
        "suffix_punctuation",
    ]
    group_value_names = {
        "space_prefix": ["no_space_prefix", "with_space_prefix"],
        "base_capitalization": ["no_capitalization", "capitalize_first"],
        "determiners": ["no_determiner", "det_the"],
        "article_capitalization": ["no_article_capitalization", "capitalize_article"],
        "prepositions": ["no_preposition", "prep_on"],
        "prep_capitalization": ["no_prep_capitalization", "capitalize_preposition"],
        "prefix_punctuation": ["no_prefix_punctuation", "punct_prefix_("],
        "suffix_punctuation": ["no_suffix_punctuation", "punct_suffix_."],
    }
    literal_maps = {
        "determiners": {"the": {"group_name": "determiners", "rel_idx": 1}},
        "prepositions": {"on": {"group_name": "prepositions", "rel_idx": 1}},
        "prefix_punctuation": {
            "(": {"group_name": "prefix_punctuation", "rel_idx": 1}
        },
        "suffix_punctuation": {
            ".": {"group_name": "suffix_punctuation", "rel_idx": 1}
        },
    }
    mergeable_ranks = [
        {"token": bytes([token_id]).decode("latin-1"), "rank": token_id}
        for token_id in range(256)
    ]
    mergeable_ranks.append({"token": " dog", "rank": 256})

    return {
        "version": 1,
        "num_modifier_groups": len(group_names),
        "modifier_group_sizes": [2] * len(group_names),
        "default_modifier": [0] * len(group_names),
        "group_names": group_names,
        "group_value_names": group_value_names,
        "entries": [],
        "reverse_entries": [],
        "token_meta": [],
        "runtime": {
            "group_indices": {
                name: group_idx for group_idx, name in enumerate(group_names)
            },
            "literal_maps": literal_maps,
            "multi_token_first_group_indices": [0, 2, 4],
            "attachment_limits": {
                "max_prefix_punctuation": 1,
                "max_suffix_punctuation": 1,
            },
        },
        "base_bpe": {
            "pattern": r"(?s).+",
            "mergeable_ranks": mergeable_ranks,
            "special_tokens": {},
        },
    }


def test_compositional_tokenizer_modifier_byte_lengths_match_decode():
    tokenizer = rustbpe.CompositionalTokenizer(json.dumps(_config()))
    base_id = 256
    rows = [
        [0, 0, 0, 0, 0, 0, 0, 0],
        [1, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 1, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 1, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 1, 0],
        [0, 0, 0, 0, 0, 0, 0, 1],
        [1, 1, 1, 1, 1, 1, 1, 1],
    ]

    byte_lengths = tokenizer.utf8_len_with_modifiers_batch(
        [base_id] * len(rows), rows
    )
    decoded = [
        tokenizer.decode_with_modifiers([base_id], [row]) for row in rows
    ]

    assert byte_lengths == [len(surface.encode("utf-8")) for surface in decoded]
    assert decoded[0] == " dog"
    assert decoded[5] == "dog."
    assert byte_lengths[5] == 4
