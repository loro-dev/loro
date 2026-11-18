//! `get_entity_index_for_text_insert` can start from the cursor cached by an
//! earlier lookup. That shortcut must never move an insert across a style
//! anchor, so these tests compare every cached answer with a cold lookup
//! (`find_best_insert_pos`) on documents full of anchors.

use loro_common::LoroValue;
use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};

use super::{PosType, RichtextState};
use crate::{
    container::richtext::{
        config::{StyleConfig, StyleConfigMap},
        ExpandType,
    },
    handler::{HandlerTrait, TextHandler},
    LoroDoc,
};

/// One key per expand type.
const STYLES: [(&str, ExpandType); 4] = [
    ("after", ExpandType::After),
    ("before", ExpandType::Before),
    ("both", ExpandType::Both),
    ("none", ExpandType::None),
];

/// BMP, astral, ZWJ sequences, flags, and modifiers.
const SNIPPETS: [&str; 9] = ["a", "bc", "é", "中", "😀", "𝒳y", "👍🏽", "👨‍👩‍👧", "🇨🇳"];

const POS_TYPES: [PosType; 5] = [
    PosType::Bytes,
    PosType::Unicode,
    PosType::Utf16,
    PosType::Event,
    PosType::Entity,
];

const MAX_LEN: usize = 40;

fn random_edit(rng: &mut StdRng, text: &TextHandler) {
    let len = text.len_unicode();
    match rng.gen_range(0..10) {
        0..=3 if len < MAX_LEN => {
            let s = SNIPPETS.choose(rng).unwrap();
            text.insert(rng.gen_range(0..=len), s, PosType::Unicode)
                .unwrap();
        }
        0..=4 if len > 0 => {
            let pos = rng.gen_range(0..len);
            let del = rng.gen_range(1..=(len - pos).min(4));
            text.delete(pos, del, PosType::Unicode).unwrap();
        }
        _ if len > 0 => {
            let start = rng.gen_range(0..len);
            let end = rng.gen_range(start + 1..=len);
            let (key, _) = *STYLES.choose(rng).unwrap();
            let value = match rng.gen_range(0..5) {
                0 => LoroValue::Null,
                1 => false.into(),
                2 => "v".into(),
                _ => true.into(),
            };
            if rng.gen_bool(0.2) {
                text.unmark(start, end, key, PosType::Unicode).unwrap();
            } else {
                text.mark(start, end, key, value, PosType::Unicode).unwrap();
            }
            if rng.gen_bool(0.2) {
                // Leaves the anchors of the style next to each other.
                text.delete(start, end - start, PosType::Unicode).unwrap();
            }
        }
        _ => text.insert(0, "a", PosType::Unicode).unwrap(),
    }
}

fn boundaries(text: &str, pos_type: PosType, entity_len: usize) -> Vec<usize> {
    let unit_len = |c: char| match pos_type {
        PosType::Bytes => c.len_utf8(),
        PosType::Unicode => 1,
        PosType::Utf16 => c.len_utf16(),
        PosType::Event if cfg!(feature = "wasm") => c.len_utf16(),
        PosType::Event => 1,
        PosType::Entity => unreachable!(),
    };
    if pos_type == PosType::Entity {
        return (0..=entity_len).collect();
    }
    let mut ans = vec![0];
    for c in text.chars() {
        ans.push(ans.last().unwrap() + unit_len(c));
    }
    ans
}

fn cold_lookup(state: &RichtextState, pos: usize, pos_type: PosType) -> usize {
    let mut cold = state.clone();
    cold.clear_cache();
    cold.get_entity_index_for_text_insert(pos, pos_type)
        .unwrap()
        .0
}

fn assert_cache_matches_cold_lookup(state: &mut RichtextState, text: &str, context: &str) {
    for pos_type in POS_TYPES {
        let positions = boundaries(text, pos_type, state.len_entity());
        // Starting from the cursor the last edit left behind.
        for &pos in &positions {
            let expected = cold_lookup(state, pos, pos_type);
            let (actual, _) = state
                .clone()
                .get_entity_index_for_text_insert(pos, pos_type)
                .unwrap();
            assert_eq!(
                actual, expected,
                "{context}: {pos_type:?} {pos} from the edit's cursor in {text:?}"
            );
        }
        // Starting from the cursor of the previous position, as when typing.
        let mut warm = state.clone();
        for &pos in &positions {
            let expected = cold_lookup(&warm, pos, pos_type);
            let (actual, cursor) = warm
                .get_entity_index_for_text_insert(pos, pos_type)
                .unwrap();
            assert_eq!(
                actual, expected,
                "{context}: {pos_type:?} {pos} from the previous position in {text:?}"
            );
            if let Some(cursor) = cursor {
                assert_eq!(
                    warm.get_index_from_cursor(cursor, PosType::Entity),
                    Some(actual)
                );
            }
        }
    }
}

#[test]
fn cached_insert_position_matches_cold_lookup_near_style_anchors() {
    let seeds: u64 = std::env::var("LORO_INSERT_CACHE_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(12);
    for seed in 0..seeds {
        let mut rng = StdRng::seed_from_u64(seed);
        let doc = LoroDoc::new_auto_commit();
        let mut styles = StyleConfigMap::new();
        for (key, expand) in STYLES {
            styles.insert(key.into(), StyleConfig::new().expand(expand));
        }
        doc.config_text_style(styles);
        let text = doc.get_text("text");
        for step in 0..60 {
            random_edit(&mut rng, &text);
            if rng.gen_bool(0.3) {
                doc.commit_then_renew();
            }
            let value = text.to_string();
            // The clone keeps the cursor cache of the document's state.
            let mut state = doc.app_state().lock().with_state_mut(text.idx(), |s| {
                s.as_richtext_state_mut().unwrap().inner_state_mut().clone()
            });
            assert_cache_matches_cold_lookup(
                &mut state,
                &value,
                &format!("seed {seed} step {step}"),
            );
        }
    }
}

#[test]
fn delete_insert_matches_cold_cursor_and_full_replay() {
    use crate::encoding::ExportMode;

    for (seed, (_, expand)) in STYLES.into_iter().enumerate() {
        let mut rng = StdRng::seed_from_u64(seed as u64);
        let warm = LoroDoc::new_auto_commit();
        let cold = LoroDoc::new_auto_commit();
        for doc in [&warm, &cold] {
            doc.set_peer_id(1).unwrap();
            let mut styles = StyleConfigMap::new();
            styles.insert("bold".into(), StyleConfig::new().expand(expand));
            styles.insert("link".into(), StyleConfig::new().expand(expand));
            doc.config_text_style(styles);
            let text = doc.get_text("text");
            text.insert_utf16(0, "ab😀cd🚀ef").unwrap();
            text.mark(2, 6, "bold", true.into(), PosType::Utf16)
                .unwrap();
            text.mark(
                0,
                4,
                "link",
                "https://example.test/initial".into(),
                PosType::Utf16,
            )
            .unwrap();
        }

        let warm_text = warm.get_text("text");
        let cold_text = cold.get_text("text");
        for step in 0..100 {
            let value = warm_text.to_string();
            let positions = boundaries(&value, PosType::Utf16, 0);
            let start = rng.gen_range(0..positions.len() - 1);
            let end = rng.gen_range(start + 1..=usize::min(start + 3, positions.len() - 1));
            let (pos, len) = (positions[start], positions[end] - positions[start]);
            let replacement = *SNIPPETS.choose(&mut rng).unwrap();

            warm_text.delete(pos, len, PosType::Utf16).unwrap();
            cold_text.delete(pos, len, PosType::Utf16).unwrap();
            cold.app_state()
                .lock()
                .with_state_mut(cold_text.idx(), |state| {
                    state
                        .as_richtext_state_mut()
                        .unwrap()
                        .inner_state_mut()
                        .clear_cache();
                });
            warm_text.insert_utf16(pos, replacement).unwrap();
            cold_text.insert_utf16(pos, replacement).unwrap();

            if step % 5 == 0 {
                // Reintroduce style boundaries as random replacements consume
                // the original styled text.
                let end = replacement.encode_utf16().count() + pos;
                for text in [&warm_text, &cold_text] {
                    if step % 10 == 0 {
                        text.mark(pos, end, "bold", true.into(), PosType::Utf16)
                            .unwrap();
                    } else {
                        text.mark(
                            pos,
                            end,
                            "link",
                            "https://example.test/new".into(),
                            PosType::Utf16,
                        )
                        .unwrap();
                    }
                }
            }
            assert_eq!(warm_text.to_string(), cold_text.to_string());
            assert_eq!(warm_text.get_delta(), cold_text.get_delta());

            if step % 10 == 0 {
                let replay = LoroDoc::new_auto_commit();
                replay
                    .import(&warm.export(ExportMode::all_updates()).unwrap())
                    .unwrap();
                assert_eq!(warm_text.get_delta(), replay.get_text("text").get_delta());
                cold.commit_then_renew();
            }
        }
    }
}
