use std::{
    cell::{Ref, RefCell, RefMut},
    rc::{Rc, Weak as RcWeak},
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak as ArcWeak},
};

#[cfg(feature = "parallel")]
pub(crate) type Irc<T> = Arc<T>;
#[cfg(not(feature = "parallel"))]
pub(crate) type Irc<T> = Rc<T>;

#[cfg(feature = "parallel")]
pub(crate) type IsoWeak<T> = ArcWeak<T>;
#[cfg(not(feature = "parallel"))]
pub(crate) type IsoWeak<T> = RcWeak<T>;

#[cfg(feature = "parallel")]
#[derive(Debug)]
pub(crate) struct IsoRw<T>(RwLock<T>);
#[cfg(not(feature = "parallel"))]
#[derive(Debug)]
pub(crate) struct IsoRw<T>(RefCell<T>);

#[cfg(feature = "parallel")]
pub(crate) type IsoRef<'a, T> = RwLockReadGuard<'a, T>;
#[cfg(not(feature = "parallel"))]
pub(crate) type IsoRef<'a, T> = Ref<'a, T>;

#[cfg(feature = "parallel")]
pub(crate) type IsoRefMut<'a, T> = RwLockWriteGuard<'a, T>;
#[cfg(not(feature = "parallel"))]
pub(crate) type IsoRefMut<'a, T> = RefMut<'a, T>;

#[cfg(feature = "parallel")]
mod rw_parallel {
    use super::*;

    impl<T> IsoRw<T> {
        #[inline(always)]
        pub fn new(t: T) -> Self {
            Self(RwLock::new(t))
        }

        #[inline(always)]
        pub fn read(&self) -> std::sync::RwLockReadGuard<T> {
            self.0.read().unwrap()
        }

        #[inline(always)]
        pub fn write(&self) -> std::sync::RwLockWriteGuard<T> {
            self.0.write().unwrap()
        }
    }
}

#[cfg(not(feature = "parallel"))]
mod rw_single {
    use std::{cell::RefCell, ops::Deref};

    use super::IsoRw;

    impl<T> IsoRw<T> {
        #[inline(always)]
        pub fn new(t: T) -> Self {
            IsoRw(RefCell::new(t))
        }

        #[inline(always)]
        pub fn read(&self) -> std::cell::Ref<T> {
            self.0.borrow()
        }

        #[inline(always)]
        pub fn write(&self) -> std::cell::RefMut<T> {
            self.0.borrow_mut()
        }
    }
}
