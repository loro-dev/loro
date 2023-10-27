#[derive(Debug, Clone)]
pub enum LazyLoad<Src, Dst: From<Src>> {
    Src(Src),
    Dst(Dst),
}

impl<Src: Default, Dst: From<Src>> LazyLoad<Src, Dst> {
    pub fn new(src: Src) -> Self {
        LazyLoad::Src(src)
    }

    pub fn new_dst(dst: Dst) -> Self {
        LazyLoad::Dst(dst)
    }

    pub fn get_mut(&mut self) -> &mut Dst {
        match self {
            LazyLoad::Src(src) => {
                let dst = Dst::from(std::mem::take(src));
                *self = LazyLoad::Dst(dst);
                match self {
                    LazyLoad::Dst(dst) => dst,
                    _ => unreachable!(),
                }
            }
            LazyLoad::Dst(dst) => dst,
        }
    }
}
