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


def _config_with_phrase_merges():
    config = _config()
    config["base_bpe"]["mergeable_ranks"].extend(
        [
            {"token": "th", "rank": 257},
            {"token": "the", "rank": 258},
            {"token": "Th", "rank": 259},
            {"token": "The", "rank": 260},
            {"token": " p", "rank": 261},
            {"token": " pr", "rank": 262},
            {"token": " pro", "rank": 263},
            {"token": " proj", "rank": 264},
            {"token": " proje", "rank": 265},
            {"token": " projec", "rank": 266},
            {"token": " project", "rank": 267},
            {"token": "project", "rank": 268},
        ]
    )
    return config


def _config_for_runtime_edge_cases():
    config = _config()
    group_names = config["group_names"]
    group_value_names = config["group_value_names"]
    group_value_names["prepositions"] = [
        "no_preposition",
        "prep_on",
        "prep_to",
        "prep_for",
    ]
    group_value_names["prefix_punctuation"] = [
        "no_prefix_punctuation",
        'punct_prefix_"',
    ]
    group_value_names["suffix_punctuation"] = [
        "no_suffix_punctuation",
        'punct_suffix_"',
        "punct_suffix_.",
        "punct_suffix_,",
    ]
    config["modifier_group_sizes"] = [
        len(group_value_names[name]) for name in group_names
    ]
    config["runtime"]["literal_maps"]["prefix_punctuation"] = {
        '"': {"group_name": "prefix_punctuation", "rel_idx": 1},
    }
    config["runtime"]["literal_maps"]["suffix_punctuation"] = {
        '"': {"group_name": "suffix_punctuation", "rel_idx": 1},
        ".": {"group_name": "suffix_punctuation", "rel_idx": 2},
        ",": {"group_name": "suffix_punctuation", "rel_idx": 3},
    }
    config["runtime"]["literal_maps"]["prepositions"] = {
        "on": {"group_name": "prepositions", "rel_idx": 1},
        "to": {"group_name": "prepositions", "rel_idx": 2},
        "for": {"group_name": "prepositions", "rel_idx": 3},
    }
    config["base_bpe"]["mergeable_ranks"].extend(
        [
            {"token": "on", "rank": 257},
            {"token": "de", "rank": 258},
            {"token": "dec", "rank": 259},
            {"token": ',"', "rank": 260},
            {"token": '".', "rank": 261},
            {"token": "now", "rank": 262},
            {"token": "to", "rank": 263},
            {"token": "for", "rank": 264},
            {"token": "turn", "rank": 265},
            {"token": "general", "rank": 266},
        ]
    )
    return config


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


def test_compositional_tokenizer_preserves_indentation_spaces():
    tokenizer = rustbpe.CompositionalTokenizer(json.dumps(_config()))
    text = "class example:\n    def method(self):\n        return 1"

    token_ids, modifier_rows = tokenizer.process_text(text)

    assert tokenizer.decode_with_modifiers(token_ids, modifier_rows) == text


def test_compositional_tokenizer_preserves_multi_space_modifier_boundaries():
    tokenizer = rustbpe.CompositionalTokenizer(json.dumps(_config_with_phrase_merges()))
    texts = [
        "at the project",
        "at  the project",
        "at   the project",
        "the project",
        "the  project",
        "at  The project",
        "rth west brown hare projects website at  The project also has a facebook page",
    ]

    for text in texts:
        token_ids, modifier_rows = tokenizer.process_text(text)
        assert tokenizer.decode_with_modifiers(token_ids, modifier_rows) == text


def test_compositional_tokenizer_splits_suffix_punctuation_cluster():
    tokenizer = rustbpe.CompositionalTokenizer(json.dumps(_config_for_runtime_edge_cases()))

    token_ids, modifier_rows = tokenizer.process_ids([262, 261])

    assert tokenizer.decode_with_modifiers(token_ids, modifier_rows) == 'now".'
    assert token_ids == [262, 46]
    assert modifier_rows[0][-1] == 1  # punct_suffix_"
    assert modifier_rows[1] == [0] * len(modifier_rows[1])


def test_compositional_tokenizer_attaches_titlecase_fallback_preposition():
    tokenizer = rustbpe.CompositionalTokenizer(json.dumps(_config_for_runtime_edge_cases()))

    token_ids, modifier_rows = tokenizer.process_ids([
        ord("O"),
        ord("n"),
        ord(" "),
        ord("D"),
        ord("e"),
        ord("c"),
        ord("."),
    ])

    assert tokenizer.decode_with_modifiers(token_ids, modifier_rows) == "On Dec."
    assert token_ids == [259]
    row = modifier_rows[0]
    assert row[1] == 1  # base capitalization for Dec
    assert row[4] == 1  # prep_on
    assert row[5] == 1  # prep capitalization for On
    assert row[7] == 2  # punct_suffix_.


def test_compositional_tokenizer_keeps_stranded_preposition_before_preposition():
    tokenizer = rustbpe.CompositionalTokenizer(json.dumps(_config_for_runtime_edge_cases()))

    token_ids, modifier_rows = tokenizer.process_ids([
        265,  # turn
        ord(" "),
        ord("T"),
        ord("o"),
        ord(" "),
        ord("F"),
        ord("o"),
        ord("r"),
        ord(" "),
        266,  # general
    ])

    assert tokenizer.decode_with_modifiers(token_ids, modifier_rows) == "turn To For general"
    assert token_ids == [265, 263, 266]
    assert modifier_rows[1][4] == 0  # To is literal, not a detached preposition
    assert modifier_rows[2][4] == 3  # prep_for
    assert modifier_rows[2][5] == 1  # prep capitalization for For
