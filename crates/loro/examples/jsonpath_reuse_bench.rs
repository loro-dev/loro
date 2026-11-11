//! Parse reuse comparison. Outputs ordered samples, not an application benchmark.
use loro::{jsonpath::ParsedJsonPath, LoroDoc, LoroValue, ToJson};
use std::{hint::black_box, time::Instant};

fn main() {
    for records in [16, 10_000] {
        let doc = LoroDoc::new();
        let map = doc.get_map("records");
        for i in 0..records {
            map.insert(&format!("k{i:05}"), LoroValue::from(serde_json::json!({"done":i%2==0,"rank":i}))).unwrap();
        }
        doc.commit();
        for (path, expected) in [
            ("$.records.k00001.rank", 1),
            ("$.records[?@.done == true]", records / 2),
            ("$.records.*", records),
        ] {
            let parsed = ParsedJsonPath::parse(path).unwrap();
            let string_values = doc.jsonpath(path).unwrap();
            let parsed_values = doc.jsonpath_parsed(&parsed);
            assert_eq!(string_values.len(), expected);
            assert_eq!(parsed_values.len(), expected);
            let full_values = |values: Vec<loro::ValueOrContainer>| {
                values.into_iter().map(|v| v.get_deep_value().to_json_value()).collect::<Vec<_>>()
            };
            assert_eq!(full_values(string_values), full_values(parsed_values));
            // string and parse-plus-evaluate include parsing; reused excludes the
            // single persistent AST, whose parse cost is reported separately.
            let mut samples: [Vec<u128>; 3] = std::array::from_fn(|_| Vec::new());
            let parse_start = Instant::now();
            drop(black_box(ParsedJsonPath::parse(black_box(path)).unwrap()));
            let one_parse_ns = parse_start.elapsed().as_nanos();
            for sample in 0..46 {
                let order = if sample % 2 == 0 { [0, 1, 2] } else { [2, 1, 0] };
                for mode in order {
                    let start = Instant::now();
                    match mode {
                        0 => drop(black_box(doc.jsonpath(black_box(path)).unwrap())),
                        1 => drop(black_box(doc.jsonpath_parsed(black_box(&parsed)))),
                        2 => {
                            let query = ParsedJsonPath::parse(black_box(path)).unwrap();
                            drop(black_box(doc.jsonpath_parsed(black_box(&query))));
                            drop(query);
                        }
                        _ => unreachable!(),
                    }
                    let elapsed = start.elapsed().as_nanos();
                    if sample >= 5 { samples[mode].push(elapsed); }
                }
            }
            println!("records={records} path={path} results={expected} one_parse_ns={one_parse_ns}");
            for (mode, values) in samples.iter().enumerate() {
                println!("mode={} ordered_ns={values:?}", ["string", "reused", "parse_plus_evaluate"][mode]);
            }
        }
    }
}
