use loro::LoroDoc;

mod gc_test;
#[cfg(feature = "jsonpath")]
mod jsonpath_test;
mod undo_test;

fn gen_action(doc: &LoroDoc, seed: u64, mut ops_len: usize) {
    let mut rng = StdRng::seed_from_u64(seed);
    use loro::LoroValue;
    use rand::prelude::*;

    let root_map = doc.get_map("root");
    let root_list = doc.get_list("list");
    let root_text = doc.get_text("text");

    while ops_len > 0 {
        let op_type = rng.gen_range(0..5);
        match op_type {
            0 => {
                // Insert into map
                let key = format!("key_{}", rng.gen::<u32>());
                let value = LoroValue::from(rng.gen::<i32>());
                root_map.insert(&key, value).unwrap();
                ops_len -= 1;
            }
            1 => {
                // Insert into list
                let index = rng.gen_range(0..=root_list.len());
                let value = LoroValue::from(rng.gen::<i32>());
                root_list.insert(index, value).unwrap();
                ops_len -= 1;
            }
            2 => {
                // Insert into text
                let index = rng.gen_range(0..=root_text.len_unicode());
                let text = rng.gen::<char>().to_string();
                root_text.insert(index, &text).unwrap();
                ops_len -= 1;
            }
            3 => {
                // Delete from list
                if !root_list.is_empty() {
                    let index = rng.gen_range(0..root_list.len());
                    root_list.delete(index, 1).unwrap();
                    ops_len -= 1;
                }
            }
            4 => {
                // Delete from text
                if root_text.len_unicode() > 0 {
                    let index = rng.gen_range(0..root_text.len_unicode());
                    root_text.delete(index, 1).unwrap();
                    ops_len -= 1;
                }
            }
            _ => unreachable!(),
        }
    }
}
