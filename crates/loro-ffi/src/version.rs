use std::ops::Deref;

pub struct VersionVector(loro::VersionVector);

impl VersionVector {}

impl Deref for VersionVector {
    type Target = loro::VersionVector;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
