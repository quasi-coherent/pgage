use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::postgres::{PgArgumentBuffer, PgTypeInfo};
use sqlx::{Encode, Postgres, Type};
use std::collections::HashMap;

use crate::agtype::AGTYPE_VERSION;
use crate::cypher::{CypherError, CypherValue, Null, parse};

impl<T: CypherValue> CypherValue for Vec<T> {
    fn parse(value: &str) -> Result<Self, CypherError> {
        parse::list(parse::unannotated(value, None)?)?
            .into_iter()
            .map(T::parse)
            .collect()
    }
}

/// Simple scalar value in a [`Map`].
#[derive(Clone, Debug, PartialEq, PartialOrd)]
pub enum Scalar {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
}

impl CypherValue for Scalar {
    fn parse(value: &str) -> Result<Self, CypherError> {
        if let Ok(Null) = Null::parse(value) {
            return Ok(Self::Null);
        }
        if let Ok(b) = bool::parse(value) {
            return Ok(Self::Bool(b));
        }
        if let Ok(n) = i64::parse(value) {
            return Ok(Self::Integer(n));
        }
        if let Ok(x) = f64::parse(value) {
            return Ok(Self::Float(x));
        }
        String::parse(value).map(Self::String)
    }
}

impl Scalar {
    fn push_text(&self, out: &mut String) -> Result<(), serde_json::Error> {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Integer(n) => out.push_str(&n.to_string()),
            Self::Float(x) if x.is_nan() => out.push_str("NaN"),
            Self::Float(x) if *x == f64::INFINITY => out.push_str("Infinity"),
            Self::Float(x) if *x == f64::NEG_INFINITY => {
                out.push_str("-Infinity")
            },
            Self::Float(x) => out.push_str(&format!("{x:?}")),
            Self::String(s) => out.push_str(&serde_json::to_string(s)?),
        }
        Ok(())
    }
}

/// A map of Cypher scalars.
#[derive(Clone, Debug, Default)]
pub struct Map(pub HashMap<String, Scalar>);

impl CypherValue for Map {
    fn parse(value: &str) -> Result<Self, CypherError> {
        parse::object(parse::unannotated(value, None)?)?
            .into_iter()
            .map(|(k, v)| Ok((parse::string(k)?, Scalar::parse(v)?)))
            .collect::<Result<_, CypherError>>()
            .map(Self)
    }
}

impl Type<Postgres> for Map {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("agtype")
    }
}

impl Encode<'_, Postgres> for Map {
    fn encode_by_ref(
        &self,
        buf: &mut PgArgumentBuffer,
    ) -> Result<IsNull, BoxDynError> {
        let mut text = String::from("{");
        for (i, (k, v)) in self.0.iter().enumerate() {
            if i > 0 {
                text.push_str(", ");
            }
            text.push_str(&serde_json::to_string(k)?);
            text.push_str(": ");
            v.push_text(&mut text)?;
        }
        text.push('}');
        buf.push(AGTYPE_VERSION);
        buf.extend_from_slice(text.as_bytes());
        Ok(IsNull::No)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(m: &Map) -> String {
        let mut buf = PgArgumentBuffer::default();
        let _ = m.encode_by_ref(&mut buf).unwrap();
        let [AGTYPE_VERSION, rest @ ..] = &buf[..] else {
            panic!("missing version byte");
        };
        String::from_utf8(rest.to_vec()).unwrap()
    }

    #[test]
    fn vecs() {
        assert_eq!(
            Vec::<i64>::parse("[1, 2::integer, -3]").unwrap(),
            [1, 2, -3]
        );
        assert!(Vec::<i64>::parse("[]").unwrap().is_empty());
        assert_eq!(
            Vec::<Vec<String>>::parse(r#"[["a"], [], ["b", "c"]]"#).unwrap(),
            [vec!["a"], vec![], vec!["b", "c"]]
        );
        assert!(matches!(
            Vec::<i64>::parse(r#"[1, "a"]"#),
            Err(CypherError::InvalidInteger)
        ));
        assert!(matches!(
            Vec::<i64>::parse("[1]::path"),
            Err(CypherError::UnexpectedAnnotation(a)) if a == "path"
        ));
        assert!(matches!(Vec::<i64>::parse("1"), Err(CypherError::Syntax)));
    }

    #[test]
    fn maps() {
        let m = Map::parse(
            r#"{"n": null, "b": true, "i": -1, "f": 1.5, "g": 2::float, "s": "x\"y", "k\n": NaN}"#,
        )
        .unwrap();
        assert_eq!(m.0.len(), 7);
        assert_eq!(m.0["n"], Scalar::Null);
        assert_eq!(m.0["b"], Scalar::Bool(true));
        assert_eq!(m.0["i"], Scalar::Integer(-1));
        assert_eq!(m.0["f"], Scalar::Float(1.5));
        assert_eq!(m.0["g"], Scalar::Float(2.0));
        assert_eq!(m.0["s"], Scalar::String("x\"y".into()));
        assert!(matches!(m.0["k\n"], Scalar::Float(x) if x.is_nan()));
        assert!(Map::parse("{}").unwrap().0.is_empty());
    }

    #[test]
    fn invalid_maps() {
        assert!(matches!(
            Map::parse(r#"{"a": 1.5::numeric}"#),
            Err(CypherError::UnexpectedAnnotation(a)) if a == "numeric"
        ));
        assert!(matches!(
            Map::parse(r#"{"a": [1]}"#),
            Err(CypherError::Syntax)
        ));
        assert!(matches!(Map::parse("[1]"), Err(CypherError::Syntax)));
    }

    #[test]
    fn encode_map() {
        assert_eq!(text(&Map::default()), "{}");
        for (v, s) in [
            (Scalar::Null, "null"),
            (Scalar::Bool(false), "false"),
            (Scalar::Integer(-7), "-7"),
            (Scalar::Float(1.0), "1.0"),
            (Scalar::Float(1e20), "1e20"),
            (Scalar::Float(f64::NAN), "NaN"),
            (Scalar::Float(f64::INFINITY), "Infinity"),
            (Scalar::Float(f64::NEG_INFINITY), "-Infinity"),
            (Scalar::String("a\"b\n".into()), r#""a\"b\n""#),
        ] {
            let m = Map(HashMap::from([("k\"".to_string(), v)]));
            assert_eq!(text(&m), format!(r#"{{"k\"": {s}}}"#));
        }
    }

    #[test]
    fn encode_round_trip() {
        let m = Map(HashMap::from([
            ("a".to_string(), Scalar::Float(3.0)),
            ("b".to_string(), Scalar::String("asdf".into())),
            ("c".to_string(), Scalar::Integer(3)),
        ]));
        assert_eq!(Map::parse(&text(&m)).unwrap().0, m.0);
    }
}
