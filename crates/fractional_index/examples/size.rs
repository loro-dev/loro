use loro_fractional_index::FractionalIndex;

fn find_common_prefix(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b.iter()).take_while(|(a, b)| a == b).count()
}

fn main() {
    // jitter
    let mut rng = rand::thread_rng();
    let mut size = 0;
    let mut compress_size = 0;
    for jitter in 0..3 {
        let mut n = FractionalIndex::jitter_default(&mut rng, jitter);
        for _ in 0..10000 {
            let new = FractionalIndex::new_before_jitter(&n, &mut rng, jitter);
            assert!(new < n);
            size += new.as_bytes().len();
            compress_size +=
                new.as_bytes().len() - find_common_prefix(new.as_bytes(), n.as_bytes()) + 1;
            n = new;
        }
        println!("size = {size} compress {compress_size} with jitter {jitter}");
    }
}
