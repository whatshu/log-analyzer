use pyo3::exceptions::PyRuntimeError;
use pyo3::PyErr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LogAnalyzerError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Invalid regex pattern: {0}")]
    Regex(#[from] regex::Error),

    #[error("Repository error: {0}")]
    Repo(String),

    #[error("Operator error: {0}")]
    Operator(String),

    #[error("Line {0} out of range (total: {1})")]
    LineOutOfRange(usize, usize),

    #[error("No operations to undo")]
    NoOperationsToUndo,

    #[error("Compression error: {0}")]
    Compression(String),
}

impl From<LogAnalyzerError> for PyErr {
    fn from(err: LogAnalyzerError) -> PyErr {
        PyRuntimeError::new_err(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, LogAnalyzerError>;
