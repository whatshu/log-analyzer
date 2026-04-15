pub mod engine;
pub mod error;
pub mod index;
pub mod operator;
pub mod repo;

mod bindings;

use pyo3::prelude::*;

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<bindings::PyLogRepo>()?;
    m.add_class::<bindings::PyRepoMetadata>()?;
    m.add_class::<bindings::PyOperationRecord>()?;
    m.add_class::<bindings::PyLogStats>()?;
    m.add_class::<bindings::PyWorkspace>()?;
    Ok(())
}
