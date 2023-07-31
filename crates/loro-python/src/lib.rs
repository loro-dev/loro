use std::sync::Arc;

use loro_internal::{LoroDoc, TextHandler};
use pyo3::prelude::*;

#[pyclass]
struct Loro(LoroDoc);

#[pyclass]
struct LoroText(TextHandler);

#[pymethods]
impl Loro {
    #[new]
    pub fn __new__() -> Self {
        Self(LoroDoc::default())
    }

    pub fn get_text(&mut self, id: &str) -> LoroText {
        let text = self.0.get_text(id);
        LoroText(text)
    }
}

#[pymethods]
impl LoroText {
    pub fn insert(&mut self, ctx: &Loro, pos: usize, value: &str) -> PyResult<()> {
        self.0
            .insert(&mut ctx.0.txn().unwrap(), pos, value)
            .unwrap();
        Ok(())
    }

    pub fn value(&self) -> String {
        Arc::try_unwrap(self.0.get_value().into_string().unwrap()).unwrap_or_else(|x| (*x).clone())
    }
}

#[pymodule]
fn pyloro(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<Loro>()?;
    m.add_class::<LoroText>()?;
    Ok(())
}
