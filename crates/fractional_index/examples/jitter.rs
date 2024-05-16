fn calculate(d: usize, n: usize) -> f64 {
    let d = d as f64;
    let n = n as f64;
    let e = (-n * (n - 1.)) / (2. * d);
    1. - e.exp()
}

fn main() {
    for jitter in 1..6 {
        let mut k = 1;
        loop {
            let c = calculate(256usize.pow(jitter), k);
            if c > 0.01 {
                break;
            }
            k += 1;
        }
        println!("k = {}, jitter = {}", k, jitter);
    }
}
