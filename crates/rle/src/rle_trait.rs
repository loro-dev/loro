pub trait Mergable<Cfg = ()> {
    fn is_mergable(&self, _other: &Self, _conf: &Cfg) -> bool
    where
        Self: Sized,
    {
        false
    }

    fn merge(&mut self, _other: &Self, _conf: &Cfg)
    where
        Self: Sized,
    {
        unreachable!()
    }
}

pub trait Sliceable {
    fn slice(&self, from: usize, to: usize) -> Self;
}

#[derive(Debug, Clone, Copy)]
pub struct Slice<'a, T> {
    pub value: &'a T,
    pub start: usize,
    pub end: usize,
}

impl<T: Sliceable> Slice<'_, T> {
    pub fn into_inner(&self) -> T {
        self.value.slice(self.start, self.end)
    }
}

pub trait HasLength {
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn len(&self) -> usize;
}

pub trait Rle<Cfg = ()>: HasLength + Sliceable + Mergable<Cfg> {}

impl<T: HasLength + Sliceable + Mergable<Cfg>, Cfg> Rle<Cfg> for T {}
