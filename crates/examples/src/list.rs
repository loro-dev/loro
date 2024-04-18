use loro::{LoroList, LoroMovableList};

pub trait ListTrait {
    fn insert_num(&mut self, index: usize, value: i32);
    fn delete_num(&mut self, index: usize);
    fn length(&self) -> usize;
}

pub fn append_n(list: &mut dyn ListTrait, n: usize) {
    for i in 0..n {
        list.insert_num(list.length(), i as i32);
    }
}

pub fn prepend_n(list: &mut dyn ListTrait, n: usize) {
    for i in (0..n).rev() {
        list.insert_num(0, i as i32);
    }
}

const MULTIPLIER: u64 = 1664525;
const INCREMENT: u64 = 1013904223;
const MODULUS: u64 = 2_147_483_647;

fn lcg(seed: u64) -> (f64, u64) {
    let new_seed = (MULTIPLIER * seed + INCREMENT) % MODULUS;
    let random_number = new_seed as f64 / MODULUS as f64; // 转换为 [0, 1) 范围内的浮点数
    (random_number, new_seed)
}

pub fn random_insert(list: &mut dyn ListTrait, n: usize, mut seed: u64) {
    for i in 0..n {
        let (rand, s) = lcg(seed);
        seed = s;
        let pos = (rand * list.length() as f64).round() as usize;
        list.insert_num(pos, i as i32);
    }
}

pub fn random_delete(list: &mut dyn ListTrait, n: usize, mut seed: u64) {
    for _ in 0..n {
        let (rand, s) = lcg(seed);
        seed = s;
        let pos = (rand * list.length() as f64) as usize;
        list.delete_num(pos);
    }
}

impl ListTrait for LoroList {
    fn insert_num(&mut self, index: usize, value: i32) {
        self.insert(index, value).unwrap();
    }

    fn delete_num(&mut self, index: usize) {
        self.delete(index, 1).unwrap();
    }

    fn length(&self) -> usize {
        self.len()
    }
}

impl ListTrait for LoroMovableList {
    fn insert_num(&mut self, index: usize, value: i32) {
        self.insert(index, value).unwrap();
    }

    fn delete_num(&mut self, index: usize) {
        self.delete(index, 1).unwrap();
    }

    fn length(&self) -> usize {
        self.len()
    }
}
