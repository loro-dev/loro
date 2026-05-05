use loro_fractional_index::FractionalIndex;
use rand::{Error, RngCore};
use serde_json::json;
use std::panic::{catch_unwind, set_hook, take_hook, AssertUnwindSafe};

struct ConstantByteRng(u8);

impl RngCore for ConstantByteRng {
    fn next_u32(&mut self) -> u32 {
        u32::from(self.0) * 0x0101_0101
    }

    fn next_u64(&mut self) -> u64 {
        u64::from(self.0) * 0x0101_0101_0101_0101
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        dest.fill(self.0);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

fn idx(hex: &str) -> FractionalIndex {
    FractionalIndex::from_hex_string(hex)
}

fn maybe_hex(value: Option<FractionalIndex>) -> serde_json::Value {
    match value {
        Some(value) => json!(value.to_string()),
        None => serde_json::Value::Null,
    }
}

fn maybe_hex_or_panic(value: impl FnOnce() -> Option<FractionalIndex>) -> serde_json::Value {
    let hook = take_hook();
    set_hook(Box::new(|_| {}));
    let result = catch_unwind(AssertUnwindSafe(value));
    set_hook(hook);

    match result {
        Ok(value) => json!({
            "panics": false,
            "value": maybe_hex(value),
        }),
        Err(_) => json!({
            "panics": true,
            "value": null,
        }),
    }
}

fn evenly(
    lower: Option<&FractionalIndex>,
    upper: Option<&FractionalIndex>,
    n: usize,
) -> serde_json::Value {
    match FractionalIndex::generate_n_evenly(lower, upper, n) {
        Some(values) => json!(values
            .into_iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()),
        None => serde_json::Value::Null,
    }
}

fn evenly_jitter(
    lower: Option<&FractionalIndex>,
    upper: Option<&FractionalIndex>,
    n: usize,
    jitter: u8,
    byte: u8,
) -> serde_json::Value {
    let mut rng = ConstantByteRng(byte);
    match FractionalIndex::generate_n_evenly_jitter(lower, upper, n, &mut rng, jitter) {
        Some(values) => json!(values
            .into_iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()),
        None => serde_json::Value::Null,
    }
}

fn main() {
    let default = FractionalIndex::default();
    let before = FractionalIndex::new_before(&default);
    let after = FractionalIndex::new_after(&default);

    let mut after_chain = Vec::new();
    let mut current = default.clone();
    for _ in 0..32 {
        current = FractionalIndex::new_after(&current);
        after_chain.push(current.to_string());
    }

    let mut before_chain = Vec::new();
    let mut current = default.clone();
    for _ in 0..32 {
        current = FractionalIndex::new_before(&current);
        before_chain.push(current.to_string());
    }

    let new_cases = vec![
        json!({"lower": null, "upper": null, "value": maybe_hex(FractionalIndex::new(None, None))}),
        json!({"lower": default.to_string(), "upper": null, "value": maybe_hex(FractionalIndex::new(Some(&default), None))}),
        json!({"lower": null, "upper": default.to_string(), "value": maybe_hex(FractionalIndex::new(None, Some(&default)))}),
        json!({"lower": before.to_string(), "upper": default.to_string(), "value": maybe_hex(FractionalIndex::new(Some(&before), Some(&default)))}),
        json!({"lower": default.to_string(), "upper": after.to_string(), "value": maybe_hex(FractionalIndex::new(Some(&default), Some(&after)))}),
        json!({"lower": default.to_string(), "upper": default.to_string(), "value": maybe_hex(FractionalIndex::new(Some(&default), Some(&default)))}),
        json!({"lower": after.to_string(), "upper": default.to_string(), "value": maybe_hex(FractionalIndex::new(Some(&after), Some(&default)))}),
    ];

    let between_pairs = [
        ("80", "8180"),
        ("7F80", "80"),
        ("80", "80"),
        ("8180", "80"),
        ("7080", "9080"),
        ("7F80", "8080"),
        ("8180", "8280"),
        ("10", "1080"),
        ("8080", "80"),
        ("80", "80FF"),
        ("80FF", "8180"),
        ("0080", "0180"),
        ("FE80", "FF80"),
    ];
    let between = between_pairs
        .into_iter()
        .map(|(left, right)| {
            let left_index = idx(left);
            let right_index = idx(right);
            let result =
                maybe_hex_or_panic(|| FractionalIndex::new_between(&left_index, &right_index));
            json!({
                "left": left,
                "right": right,
                "panics": result["panics"],
                "value": result["value"],
            })
        })
        .collect::<Vec<_>>();

    let mut evenly_cases = Vec::new();
    for n in 0..=20 {
        evenly_cases.push(json!({
            "lower": null,
            "upper": null,
            "n": n,
            "value": evenly(None, None, n),
        }));
    }
    for n in 1..=12 {
        evenly_cases.push(json!({
            "lower": before.to_string(),
            "upper": after.to_string(),
            "n": n,
            "value": evenly(Some(&before), Some(&after), n),
        }));
    }
    evenly_cases.push(json!({
        "lower": default.to_string(),
        "upper": default.to_string(),
        "n": 3,
        "value": evenly(Some(&default), Some(&default), 3),
    }));
    evenly_cases.push(json!({
        "lower": after.to_string(),
        "upper": before.to_string(),
        "n": 3,
        "value": evenly(Some(&after), Some(&before), 3),
    }));

    let mut rng = ConstantByteRng(0xAB);
    let jitter_default_3 = FractionalIndex::jitter_default(&mut rng, 3);
    let mut rng = ConstantByteRng(0x01);
    let before_jitter = FractionalIndex::new_before_jitter(&default, &mut rng, 2);
    let mut rng = ConstantByteRng(0x02);
    let after_jitter = FractionalIndex::new_after_jitter(&default, &mut rng, 2);
    let mut rng = ConstantByteRng(0x03);
    let between_jitter = FractionalIndex::new_between_jitter(&default, &after, &mut rng, 2);
    let mut rng = ConstantByteRng(0x04);
    let new_jitter_none = FractionalIndex::new_jitter(None, None, &mut rng, 2);
    let mut rng = ConstantByteRng(0x05);
    let new_jitter_after = FractionalIndex::new_jitter(Some(&default), None, &mut rng, 2);
    let mut rng = ConstantByteRng(0x06);
    let new_jitter_before = FractionalIndex::new_jitter(None, Some(&default), &mut rng, 2);

    let fixture = json!({
        "basic": {
            "terminator": 128,
            "default": default.to_string(),
            "beforeDefault": before.to_string(),
            "afterDefault": after.to_string(),
            "fromHexOddLength": FractionalIndex::from_hex_string("80ffA").to_string(),
            "fromBytes": FractionalIndex::from_bytes(vec![0, 15, 128, 255]).to_string(),
        },
        "chains": {
            "after": after_chain,
            "before": before_chain,
        },
        "newCases": new_cases,
        "between": between,
        "evenly": evenly_cases,
        "jitter": {
            "defaultJitter0": {
                "jitter": 0,
                "byte": 0xAB,
                "value": FractionalIndex::jitter_default(&mut ConstantByteRng(0xAB), 0).to_string(),
            },
            "defaultJitter3": {
                "jitter": 3,
                "byte": 0xAB,
                "value": jitter_default_3.to_string(),
            },
            "before": {
                "input": default.to_string(),
                "jitter": 2,
                "byte": 0x01,
                "value": before_jitter.to_string(),
            },
            "after": {
                "input": default.to_string(),
                "jitter": 2,
                "byte": 0x02,
                "value": after_jitter.to_string(),
            },
            "between": {
                "lower": default.to_string(),
                "upper": after.to_string(),
                "jitter": 2,
                "byte": 0x03,
                "value": maybe_hex(between_jitter),
            },
            "newNoneNone": {
                "lower": null,
                "upper": null,
                "jitter": 2,
                "byte": 0x04,
                "value": maybe_hex(new_jitter_none),
            },
            "newAfter": {
                "lower": default.to_string(),
                "upper": null,
                "jitter": 2,
                "byte": 0x05,
                "value": maybe_hex(new_jitter_after),
            },
            "newBefore": {
                "lower": null,
                "upper": default.to_string(),
                "jitter": 2,
                "byte": 0x06,
                "value": maybe_hex(new_jitter_before),
            },
            "generateN": {
                "lower": null,
                "upper": null,
                "n": 7,
                "jitter": 1,
                "byte": 0xCC,
                "value": evenly_jitter(None, None, 7, 1, 0xCC),
            }
        }
    });

    println!("{}", serde_json::to_string_pretty(&fixture).unwrap());
}
