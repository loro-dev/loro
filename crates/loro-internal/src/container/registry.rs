pub use container_idx::ContainerIdx;

mod container_idx {
    use super::super::ContainerType;

    /// Inner representation for ContainerID.
    /// It contains the unique index for the container and the type of the container.
    /// It uses top 4 bits to represent the type of the container.
    ///
    /// It's only used inside this crate and should not be exposed to the user.
    ///
    /// TODO: make this type private in this crate only
    ///
    // During a transaction, we may create some containers which are deleted later. And these containers also need a unique ContainerIdx.
    // So when we encode snapshot, we need to sort the containers by ContainerIdx and change the `container` of ops to the index of containers.
    // An empty store decodes the snapshot, it will create these containers in a sequence of natural numbers so that containers and ops can correspond one-to-one
    #[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
    pub struct ContainerIdx(u32);

    impl std::fmt::Debug for ContainerIdx {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_tuple("ContainerIdx")
                .field(&self.get_type())
                .field(&self.to_index())
                .finish()
        }
    }

    impl ContainerIdx {
        pub(crate) const TYPE_MASK: u32 = 0b1111 << 28;
        pub(crate) const INDEX_MASK: u32 = !Self::TYPE_MASK;

        #[allow(unused)]
        pub(crate) fn get_type(self) -> ContainerType {
            match (self.0 & Self::TYPE_MASK) >> 28 {
                0 => ContainerType::Map,
                1 => ContainerType::List,
                2 => ContainerType::Text,
                _ => unreachable!(),
            }
        }

        #[allow(unused)]
        pub(crate) fn to_index(self) -> u32 {
            self.0 & Self::INDEX_MASK
        }

        pub(crate) fn from_index_and_type(index: u32, container_type: ContainerType) -> Self {
            let prefix: u32 = match container_type {
                ContainerType::Map => 0,
                ContainerType::List => 1,
                ContainerType::Text => 2,
            } << 28;

            Self(prefix | index)
        }
    }
}
