use examples::utils;
use loro::LoroDoc;

pub fn main() {
    let doc = LoroDoc::new();
    const N: usize = 100_000;
    let text = doc.get_text("text");
    println!(
        "Task: Insert {} random strings (length 10) at random positions into text",
        N
    );

    for _ in 0..N {
        let random_string: String = (0..10)
            .map(|_| (b'a' + rand::random::<u8>() % 26) as char)
            .collect();
        let random_position = rand::random::<usize>() % (text.len_unicode() + 1);
        text.insert(random_position, &random_string).unwrap();
    }

    utils::bench_fast_snapshot(&doc);
}
