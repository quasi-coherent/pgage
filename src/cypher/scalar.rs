use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::postgres::{PgArgumentBuffer, PgTypeInfo};
use sqlx::{Encode, Postgres, Type};

use crate::agtype::AGTYPE_VERSION;
use crate::cypher::{CypherError, CypherValue, parse};

/// The Cypher `null` value.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Null;

impl CypherValue for Null {
    fn parse(value: &str) -> Result<Self, CypherError> {
        match parse::unannotated(value, None)? {
            "null" => Ok(Self),
            _ => Err(CypherError::InvalidNull),
        }
    }
}

impl Type<Postgres> for Null {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("agtype")
    }
}

impl Encode<'_, Postgres> for Null {
    fn encode_by_ref(
        &self,
        buf: &mut PgArgumentBuffer,
    ) -> Result<IsNull, BoxDynError> {
        buf.push(AGTYPE_VERSION);
        buf.extend_from_slice("null".as_bytes());
        Ok(IsNull::No)
    }
}

impl CypherValue for bool {
    fn parse(value: &str) -> Result<Self, CypherError> {
        match parse::unannotated(value, None)? {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(CypherError::InvalidBool),
        }
    }
}

impl CypherValue for String {
    fn parse(value: &str) -> Result<Self, CypherError> {
        parse::string(parse::unannotated(value, None)?)
    }
}

macro_rules! integer {
    ($($t:ty),*) => {$(
        impl CypherValue for $t {
            fn parse(value: &str) -> Result<Self, CypherError> {
                parse::unannotated(value, Some("integer"))?
                    .parse()
                    .map_err(|_| CypherError::InvalidInteger)
            }
        }
    )*};
}

integer!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

macro_rules! float {
    ($($t:ty),*) => {$(
        impl CypherValue for $t {
            fn parse(value: &str) -> Result<Self, CypherError> {
                parse::unannotated(value, Some("float"))?
                    .parse()
                    .map_err(|_| CypherError::InvalidFloat)
            }
        }
    )*};
}

float!(f32, f64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null() {
        assert!(matches!(Null::parse("null"), Ok(Null)));
        assert!(matches!(Null::parse("1"), Err(CypherError::InvalidNull)));
        assert!(matches!(Null::parse(""), Err(CypherError::Syntax)));
    }

    #[test]
    fn bools() {
        assert!(bool::parse("true").unwrap());
        assert!(!bool::parse(" false ").unwrap());
        assert!(matches!(bool::parse("1"), Err(CypherError::InvalidBool)));
    }

    #[test]
    fn strings() {
        assert_eq!(String::parse(r#""a\"b\n""#).unwrap(), "a\"b\n");
        assert!(matches!(String::parse("1"), Err(CypherError::Syntax)));
        assert!(matches!(
            String::parse(r#""a"::vertex"#),
            Err(CypherError::UnexpectedAnnotation(a)) if a == "vertex"
        ));
    }

    #[test]
    fn integers() {
        assert_eq!(i64::parse("-42").unwrap(), -42);
        assert_eq!(i64::parse("42::integer").unwrap(), 42);
        assert_eq!(i64::parse(" 1 ").unwrap(), 1);
        assert_eq!(i8::parse("127").unwrap(), 127);
        assert_eq!(i32::parse("-2147483648").unwrap(), i32::MIN);
        assert!(matches!(i8::parse("128"), Err(CypherError::InvalidInteger)));
        assert_eq!(isize::parse("-5::integer").unwrap(), -5);
        assert_eq!(u8::parse("255").unwrap(), 255);
        assert_eq!(u16::parse("65535").unwrap(), u16::MAX);
        assert_eq!(u32::parse("7::integer").unwrap(), 7);
        assert_eq!(u64::parse("9223372036854775807").unwrap(), i64::MAX as u64);
        assert_eq!(usize::parse("0").unwrap(), 0);
        assert!(matches!(u8::parse("256"), Err(CypherError::InvalidInteger)));
        assert!(matches!(u64::parse("-1"), Err(CypherError::InvalidInteger)));
        assert!(matches!(usize::parse("-1"), Err(CypherError::InvalidInteger)));
        assert!(matches!(
            i64::parse("99999999999999999999"),
            Err(CypherError::InvalidInteger)
        ));
        assert!(matches!(i64::parse("1.5"), Err(CypherError::InvalidInteger)));
        assert!(matches!(
            i64::parse(r#""1""#),
            Err(CypherError::InvalidInteger)
        ));
        assert!(matches!(
            i64::parse("1::float"),
            Err(CypherError::UnexpectedAnnotation(a)) if a == "float"
        ));
    }

    #[test]
    fn floats() {
        assert_eq!(f64::parse("1.0").unwrap(), 1.0);
        assert_eq!(f64::parse("-1.5").unwrap(), -1.5);
        assert_eq!(f64::parse("1e3").unwrap(), 1000.0);
        assert_eq!(f64::parse("2").unwrap(), 2.0);
        assert_eq!(f64::parse("2::float").unwrap(), 2.0);
        assert_eq!(f32::parse("0.5").unwrap(), 0.5);
        assert!(f64::parse("NaN").unwrap().is_nan());
        assert_eq!(f64::parse("Infinity").unwrap(), f64::INFINITY);
        assert_eq!(f64::parse("-Infinity").unwrap(), f64::NEG_INFINITY);
        assert!(matches!(f64::parse("true"), Err(CypherError::InvalidFloat)));
        assert!(matches!(
            f64::parse("1.5::numeric"),
            Err(CypherError::UnexpectedAnnotation(a)) if a == "numeric"
        ));
    }
}
