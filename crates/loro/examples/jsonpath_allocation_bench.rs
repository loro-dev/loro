use loro::{LoroDoc, LoroValue};
use std::{hint::black_box, time::Instant};

fn main() {
    let doc = LoroDoc::new();
    let records = doc.get_map("records");
    for i in 0..10_000 {
        records
            .insert(
                &format!("k{i:05}"),
                LoroValue::from(serde_json::json!({"done":i%2==0,"rank":i,"nested":{"value":i}})),
            )
            .unwrap();
    }
    doc.commit();
    for (query, expected) in [
        ("$.records[?@.done == true]", 5000),
        ("$.records[?@.rank >= 9900]", 100),
        ("$.records[?@.nested.value >= 9900]", 100),
        ("$.records.*", 10_000),
        ("$.records.k00001.rank", 1),
    ] {
        assert_eq!(doc.jsonpath(query).unwrap().len(), expected);
        let mut times = Vec::new();
        for _ in 0..41 {
            let start = Instant::now();
            black_box(doc.jsonpath(black_box(query)).unwrap());
            times.push(start.elapsed().as_nanos());
        }
        times.sort_unstable();
        println!(
            "query={query} median_ns={} p95_ns={} results={expected}",
            times[20], times[38]
        );
    }
}
