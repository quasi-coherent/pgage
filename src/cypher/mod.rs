//! Conversions between Cypher types and Rust.
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::AgError;

mod composite;
pub use composite::{Map, Scalar};

mod graph;
pub use graph::{Edge, GraphId, Path, Segment, Vertex};

mod parse;

mod scalar;
pub use scalar::Null;

/// Entity values in the AGE Cypher query format.
pub trait Entity: Serialize + DeserializeOwned {
    /// Parse the entity from a label and properties object.
    fn parse(label: &str, properties: &str) -> Result<Self, AgError>;

    /// Encode this value as a parameter map to use in a prepared statement.
    fn to_parameter_map(&self) -> Result<String, AgError>;
}

pub(crate) trait CypherValue: Sized {
    fn parse(value: &str) -> Result<Self, CypherError>;
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CypherError {
    #[error("unsupported agtype binary format version")]
    InvalidVersion,
    #[error("syntax error")]
    Syntax,
    #[error("unterminated string")]
    UnterminatedString,
    #[error("invalid escape sequence")]
    InvalidEscape,
    #[error("invalid integer")]
    InvalidInteger,
    #[error("invalid float")]
    InvalidFloat,
    #[error("path must alternate vertex, edge, vertex")]
    InvalidPath,
    #[error("invalid null")]
    InvalidNull,
    #[error("invalid boolean")]
    InvalidBool,
    #[error("unexpected annotation `::{0}`")]
    UnexpectedAnnotation(String),
    #[error("missing annotation `::{0}`")]
    MissingAnnotation(&'static str),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Serde(Box<dyn std::error::Error + Send + Sync>),
}

impl From<AgError> for CypherError {
    fn from(value: AgError) -> Self {
        Self::Serde(Box::new(value))
    }
}
