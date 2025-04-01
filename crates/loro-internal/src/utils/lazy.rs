#[derive(Debug, Clone)]
pub enum LazyLoad<Src, Dst: From<Src>> {
    Src(Src),
    Dst(Dst),
}

impl<Src: Default, Dst: From<Src>> LazyLoad<Src, Dst> {
    pub fn get_mut(&mut self) -> &mut Dst {
        match self {
            Self::Src(src) => {
                let dst = Dst::from(std::mem::take(src));
                *self = Self::Dst(dst);
                match self {
                    Self::Dst(dst) => dst,
                    _ => unreachable!(),
                }
            }
            Self::Dst(dst) => dst,
        }
    }
}
