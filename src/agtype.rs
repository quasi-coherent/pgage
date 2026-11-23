//! The AGE `agtype`.
use sqlx::error::BoxDynError;
use sqlx::postgres::{PgTypeInfo, PgValueFormat, PgValueRef};
use sqlx::{Decode, Postgres, Type};
use std::ops::{Deref, DerefMut};

use crate::cypher::{CypherError, CypherValue};

// Version byte that starts the binary format of an `agtype`.
#[doc(hidden)]
pub const AGTYPE_VERSION: u8 = 1;

/// Value for `agtype` fields.
///
/// Every AGE query returns a `agtype`.  `AgType` will attempt to cast to the
/// typed passed in as the generic.
///
/// # Example
///
/// Deriving `Entity` makes a type suitable to be the properties object of an
/// entity.
///
/// ```rust,no_run
/// # async fn f() {
/// use pgage::cypher::Vertex;
/// use pgage::{AgType, Entity};
/// use serde::{Deserialize, Serialize};
/// use sqlx::PgPool;
///
/// /// Properties of a `person` node.
/// #[derive(Clone, Debug, Deserialize, Entity, Eq, PartialEq, Serialize)]
/// struct Person {
///     name: String,
///     age: u32,
///     email: Option<String>,
/// }
///
/// let pool = PgPool::connect("postgresql://postgres:postgres@localhost:5432")
///     .await
///     .unwrap();
///
/// let person = Person {
///     name: "Bartholomew".into(),
///     age: 66,
///     email: Some("bart@apostles.ai".into()),
/// };
///
/// /// Transactional AGE queries are not (presently) compatible with sqlx
/// /// macros.
/// /// We have to use non-macro versions for things like `CREATE`:
/// sqlx::query(
///     r#"
///   SELECT * FROM cypher(
///     'example_graph',
///     $$ CREATE (:Person {name: $name, age: $age, email: $email}) $$,
///     $1
///   ) AS (u agtype);
/// "#,
/// )
/// .bind(&person)
/// .execute(&pool)
/// .await
/// .unwrap();
///
/// /// Read-only queries are supported for sqlx compile-time macros:
/// let mut vs = sqlx::query_as!(
///     AgType<Vertex<Person>>,
///     r#"
///   SELECT * FROM cypher(
///     'example_graph',
///     $$ MATCH (a: Person) WHERE a.name == "Bartholomew" RETURN a $$
///   ) as (a agtype);
/// "#,
/// )
/// .fetch_all(&pool)
/// .await
/// .unwrap();
///
/// let v = vs.pop().unwrap().v();
/// assert!(vs.pop().is_none());
/// assert_eq!(v, person);
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AgType<T: ?Sized>(pub T);

impl<T> AgType<T> {
    /// Extract the inner value.
    pub fn v(self) -> T {
        self.0
    }
}

impl<T> From<T> for AgType<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T> Deref for AgType<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for AgType<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> AsRef<T> for AgType<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}

impl<T> AsMut<T> for AgType<T> {
    fn as_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: ?Sized> Type<Postgres> for AgType<T> {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("agtype")
    }
}

impl<'r, T: CypherValue> Decode<'r, Postgres> for AgType<T> {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        let text = match value.format() {
            PgValueFormat::Text => value.as_str()?,
            PgValueFormat::Binary => match value.as_bytes()? {
                [AGTYPE_VERSION, rest @ ..] => std::str::from_utf8(rest)?,
                _ => return Err(CypherError::InvalidVersion.into()),
            },
        };
        let v = T::parse(text)?;
        Ok(AgType(v))
    }
}
