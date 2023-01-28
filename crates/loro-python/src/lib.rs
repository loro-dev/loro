use std::ops::{Deref, DerefMut};

use loro_internal::{LoroCore, Text};
use pyo3::prelude::*;

#[pyclass]
struct Loro(LoroCore);

#[pyclass]
struct LoroText(Text);

impl Deref for Loro {
    type Target = LoroCore;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Loro {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Deref for LoroText {
    type Target = Text;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for LoroText {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[pymethods]
impl Loro {
    #[new]
    pub fn __new__() -> Self {
        Self(LoroCore::default())
    }

    pub fn get_text(&mut self, id: &str) -> LoroText {
        let text = self.0.get_text(id);
        LoroText(text)
    }
}

#[pymethods]
impl LoroText {
    pub fn insert(&mut self, ctx: &Loro, pos: usize, value: &str) -> PyResult<()> {
        self.0.insert(&ctx.0, pos, value).unwrap();
        Ok(())
    }

    pub fn value(&self) -> String {
        self.0.get_value().into_string().unwrap().into_string()
    }
}

#[pymodule]
fn pyloro(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<Loro>()?;
    m.add_class::<LoroText>()?;
    Ok(())
}
