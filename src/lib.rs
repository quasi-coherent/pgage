//! # `pgage`
//!
//! Query PostgreSQL with the AGE extension in Rust.
pub mod agtype;
#[doc(inline)]
pub use agtype::AgType;

pub mod cypher;

extern crate pgage_derive;

#[doc(hidden)]
pub use pgage_derive::Entity;

/// Errors that can occur in the course of an operation.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct AgError(#[from] pub(crate) AgErrorImpl);

impl AgError {
    /// New error value.
    pub fn new<T: std::fmt::Display>(msg: T) -> Self {
        <Self as serde::de::Error>::custom(msg)
    }
}

impl serde::de::Error for AgError {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        AgErrorImpl::Custom(msg.to_string()).into()
    }

    /// An entity's label doesn't match any of the expected labels.
    fn unknown_variant(
        variant: &str,
        expected: &'static [&'static str],
    ) -> Self {
        AgErrorImpl::UnknownLabel { found: variant.into(), expected }.into()
    }
}

#[derive(Debug, thiserror::Error)]
enum AgErrorImpl {
    #[error("uknonwn label `{found}`, expected one of {expected:?}")]
    UnknownLabel { found: String, expected: &'static [&'static str] },
    #[error("{0}")]
    Custom(String),
}

// Proc macro module paths:
#[doc(hidden)]
pub mod private {
    pub extern crate serde;
    pub extern crate serde_json;
    pub extern crate sqlx;
    pub use super::agtype::AGTYPE_VERSION;
}
