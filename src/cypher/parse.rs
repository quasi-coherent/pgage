use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::character::complete::{alpha1, char, multispace0, one_of};
use nom::combinator::{all_consuming, opt, recognize};
use nom::error::ErrorKind;
use nom::multi::{many0, many1, separated_list0};
use nom::sequence::{delimited, preceded, separated_pair, terminated};
use nom::{IResult, Parser};
use std::str::Chars;

use crate::cypher::CypherError;

pub(crate) fn annotated<'a>(
    input: &'a str,
    name: &'static str,
) -> Result<&'a str, CypherError> {
    match complete(input, (body, opt(annotation)))? {
        (body, Some(a)) if a == name => Ok(body),
        (_, Some(a)) => Err(CypherError::UnexpectedAnnotation(a.into())),
        (_, None) => Err(CypherError::MissingAnnotation(name)),
    }
}

pub(crate) fn unannotated<'a>(
    input: &'a str,
    allowed: Option<&'static str>,
) -> Result<&'a str, CypherError> {
    match complete(input, (body, opt(annotation)))? {
        (body, None) => Ok(body),
        (body, Some(a)) if Some(a) == allowed => Ok(body),
        (_, Some(a)) => Err(CypherError::UnexpectedAnnotation(a.into())),
    }
}

pub(crate) fn list(input: &str) -> Result<Vec<&str>, CypherError> {
    complete(input, list_items)
}

pub(crate) fn object(input: &str) -> Result<Vec<(&str, &str)>, CypherError> {
    complete(input, object_items)
}

pub(crate) fn string(input: &str) -> Result<String, CypherError> {
    complete(input, unescape)
}

struct Error {
    kind: CypherError,
}

impl Error {
    fn failure<'a, T>(kind: CypherError) -> Res<'a, T> {
        Err(nom::Err::Failure(Self { kind }))
    }
}

impl<'a> nom::error::ParseError<&'a str> for Error {
    fn from_error_kind(_: &'a str, _: ErrorKind) -> Self {
        Self { kind: CypherError::Syntax }
    }

    fn append(_: &'a str, _: ErrorKind, other: Self) -> Self {
        other
    }
}

type Res<'a, T> = IResult<&'a str, T, Error>;

fn ws<'a, O, F>(f: F) -> impl Parser<&'a str, Output = O, Error = Error>
where
    F: Parser<&'a str, Output = O, Error = Error>,
{
    delimited(multispace0, f, multispace0)
}

fn complete<'a, O, F>(input: &'a str, f: F) -> Result<O, CypherError>
where
    F: Parser<&'a str, Output = O, Error = Error>,
{
    match all_consuming(ws(f)).parse(input) {
        Ok((_, o)) => Ok(o),
        Err(nom::Err::Error(e) | nom::Err::Failure(e)) => Err(e.kind),
        Err(nom::Err::Incomplete(_)) => Err(CypherError::Syntax),
    }
}

fn value(input: &str) -> Res<'_, &str> {
    recognize((body, opt(annotation))).parse(input)
}

fn body(input: &str) -> Res<'_, &str> {
    alt((skip_string, recognize(list_items), recognize(object_items), scalar))
        .parse(input)
}

fn annotation(input: &str) -> Res<'_, &str> {
    preceded(tag("::"), alpha1).parse(input)
}

fn list_items(input: &str) -> Res<'_, Vec<&str>> {
    delimited(
        char('['),
        separated_list0(char(','), ws(value)),
        preceded(multispace0, char(']')),
    )
    .parse(input)
}

fn object_items(input: &str) -> Res<'_, Vec<(&str, &str)>> {
    delimited(
        char('{'),
        separated_list0(
            char(','),
            ws(separated_pair(skip_string, ws(char(':')), value)),
        ),
        preceded(multispace0, char('}')),
    )
    .parse(input)
}

fn scalar(input: &str) -> Res<'_, &str> {
    alt((
        tag("null"),
        tag("true"),
        tag("false"),
        tag("NaN"),
        tag("Infinity"),
        tag("-Infinity"),
        recognize((opt(char('-')), alt((float, decimal)))),
    ))
    .parse(input)
}

fn skip_string(input: &str) -> Res<'_, &str> {
    let bytes = input.as_bytes();
    if bytes.first() != Some(&b'"') {
        return Err(nom::Err::Error(Error { kind: CypherError::Syntax }));
    }
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Ok((&input[i + 1..], &input[..=i])),
            b'\\' => i += 2,
            _ => i += 1,
        }
    }
    Error::failure(CypherError::UnterminatedString)
}

fn unescape(i: &str) -> Res<'_, String> {
    let Some(body) = i.strip_prefix('"') else {
        return Err(nom::Err::Error(Error { kind: CypherError::Syntax }));
    };
    let mut out = String::new();
    let mut chars = body.chars();
    loop {
        match chars.next() {
            None => {
                return Error::failure(CypherError::UnterminatedString);
            },
            Some('"') => return Ok((chars.as_str(), out)),
            Some('\\') => match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some('b') => out.push('\u{8}'),
                Some('f') => out.push('\u{c}'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('u') => match unicode_escape(&mut chars) {
                    Some(c) => out.push(c),
                    None => {
                        return Error::failure(CypherError::InvalidEscape);
                    },
                },
                _ => return Error::failure(CypherError::InvalidEscape),
            },
            Some(c) => out.push(c),
        }
    }
}

fn unicode_escape(chars: &mut Chars<'_>) -> Option<char> {
    let hi = hex4(chars)?;
    if !(0xD800..0xDC00).contains(&hi) {
        return char::from_u32(hi);
    }
    if chars.next()? != '\\' || chars.next()? != 'u' {
        return None;
    }
    let lo = hex4(chars)?;
    if !(0xDC00..0xE000).contains(&lo) {
        return None;
    }
    char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00))
}

fn hex4(chars: &mut Chars<'_>) -> Option<u32> {
    (0..4).try_fold(0, |n, _| Some(n * 16 + chars.next()?.to_digit(16)?))
}

fn decimal(input: &str) -> Res<'_, &str> {
    recognize(many1(terminated(one_of("0123456789"), many0(char('_')))))
        .parse(input)
}

fn float(input: &str) -> Res<'_, &str> {
    alt((
        // Case one: .42
        recognize((
            char('.'),
            decimal,
            opt((one_of("eE"), opt(one_of("+-")), decimal)),
        )), // Case two: 42e42 and 42.42e42
        recognize((
            decimal,
            opt(preceded(char('.'), decimal)),
            one_of("eE"),
            opt(one_of("+-")),
            decimal,
        )), // Case three: 42. and 42.42
        recognize((decimal, char('.'), opt(decimal))),
    ))
    .parse(input)
}

// fn invalid_int<E>(_: E) -> CypherError {
//     CypherError::InvalidInteger
// }

// fn invalid_float<E>(_: E) -> CypherError {
//     CypherError::InvalidFloat
// }

// fn annot(a: &str) -> CypherError {
//     CypherError::UnexpectedAnnotation(a.into())
// }

// fn vertex(mut m: HashMap<String, AgType>) -> Result<Vertex, CypherError> {
//     Ok(Vertex {
//         id: graph_id(&mut m, "id")?,
//         label: label(&mut m)?,
//         properties: properties(&mut m)?,
//     })
// }

// fn edge(mut m: HashMap<String, AgType>) -> Result<Edge, CypherError> {
//     Ok(Edge {
//         id: graph_id(&mut m, "id")?,
//         startid: graph_id(&mut m, "start_id")?,
//         endid: graph_id(&mut m, "end_id")?,
//         label: label(&mut m)?,
//         properties: properties(&mut m)?,
//     })
// }

// fn path(l: Vec<AgType>) -> Result<Path, CypherError> {
//     let mut items = l.into_iter();
//     let Some(AgType::Vertex(start)) = items.next() else {
//         return Err(CypherError::InvalidPath);
//     };
//     let mut segments = Vec::new();
//     while let Some(e) = items.next() {
//         let (AgType::Edge(edge), Some(AgType::Vertex(vertex))) =
//             (e, items.next())
//         else {
//             return Err(CypherError::InvalidPath);
//         };
//         segments.push(Segment { edge, vertex });
//     }
//     Ok(Path { start, segments })
// }

// fn field(
//     m: &mut HashMap<String, AgType>,
//     key: &'static str,
// ) -> Result<AgType, CypherError> {
//     m.remove(key).ok_or(CypherError::MissingField(key))
// }

// fn graph_id(
//     m: &mut HashMap<String, AgType>,
//     key: &'static str,
// ) -> Result<GraphId, CypherError> {
//     match field(m, key)? {
//         AgType::Integer(n) => Ok(GraphId(n)),
//         _ => Err(CypherError::InvalidField(key)),
//     }
// }

// fn label(
//     m: &mut HashMap<String, AgType>,
// ) -> Result<std::borrow::Cow<'static, str>, CypherError> {
//     match field(m, "label")? {
//         AgType::String(s) => Ok(s),
//         _ => Err(CypherError::InvalidField("label")),
//     }
// }

// fn properties(
//     m: &mut HashMap<String, AgType>,
// ) -> Result<Properties, CypherError> {
//     match field(m, "properties")? {
//         AgType::Map(p) => Ok(Properties(p)),
//         _ => Err(CypherError::InvalidField("properties")),
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = r#"{"id": 844424930131969, "label": "Person", "properties": {"age": 29, "name": "A::edge"}}::vertex"#;
    const B: &str = r#"{"id": 844424930131970, "label": "Person", "properties": {"name": "B"}}::vertex"#;
    const KNOWS: &str = r#"{"id": 1125899906842625, "label": "KNOWS", "end_id": 844424930131970, "start_id": 844424930131969, "properties": {}}::edge"#;

    #[test]
    fn annotated_values() {
        let body = annotated(A, "vertex").unwrap();
        assert_eq!(body, A.strip_suffix("::vertex").unwrap());
        assert_eq!(annotated(&format!(" {KNOWS}\n"), "edge").unwrap(), {
            KNOWS.strip_suffix("::edge").unwrap()
        });
        assert_eq!(annotated("1::numeric", "numeric").unwrap(), "1");
    }

    #[test]
    fn wrong_annotation() {
        assert!(matches!(
            annotated(A, "edge"),
            Err(CypherError::UnexpectedAnnotation(a)) if a == "vertex"
        ));
        assert!(matches!(
            annotated(r#"{"id": 1}"#, "vertex"),
            Err(CypherError::MissingAnnotation("vertex"))
        ));
    }

    #[test]
    fn path_elements() {
        let path = format!("[{A}, {KNOWS}, {B}]::path");
        let body = annotated(&path, "path").unwrap();
        assert_eq!(list(body).unwrap(), [A, KNOWS, B]);
    }

    #[test]
    fn lists() {
        assert!(list("[]").unwrap().is_empty());
        assert!(list("[ ]").unwrap().is_empty());
        assert_eq!(
            list(r#"[1, "a,]", [2, {}], {"k": [3]}, -1.5, null, true, NaN, -Infinity, 1e3, 2::integer]"#)
                .unwrap(),
            [
                "1",
                r#""a,]""#,
                "[2, {}]",
                r#"{"k": [3]}"#,
                "-1.5",
                "null",
                "true",
                "NaN",
                "-Infinity",
                "1e3",
                "2::integer",
            ]
        );
    }

    #[test]
    fn objects() {
        assert!(object("{}").unwrap().is_empty());
        assert!(object("{ }").unwrap().is_empty());
        assert_eq!(
            object(r#"{"a": 1, "b\"": [1, 2], "c": { }}"#).unwrap(),
            [(r#""a""#, "1"), (r#""b\"""#, "[1, 2]"), (r#""c""#, "{ }")]
        );
    }

    #[test]
    fn strings() {
        assert_eq!(string(r#""a\"b\\c\n""#).unwrap(), "a\"b\\c\n");
        assert_eq!(string(r#""é😀""#).unwrap(), "é😀");
        assert!(matches!(string(r#""a\x""#), Err(CypherError::InvalidEscape)));
        assert!(matches!(
            string(r#""\ud800""#),
            Err(CypherError::InvalidEscape)
        ));
        assert!(matches!(
            string(r#""abc"#),
            Err(CypherError::UnterminatedString)
        ));
    }

    #[test]
    fn syntax_errors() {
        assert!(matches!(
            list(r#"[1, "abc"#),
            Err(CypherError::UnterminatedString)
        ));
        for s in ["", "[1, ]", "[1", "[1 2]", "1 2", "[1]]", "{1: 2}"] {
            assert!(matches!(list(s), Err(CypherError::Syntax)), "{s}");
        }
        assert!(matches!(object(r#"{"a" 1}"#), Err(CypherError::Syntax)));
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     const A: &str = r#"{"id": 844424930131969, "label": "Person",
// "properties": {"age": 29, "name": "A"}}::vertex"#;     const B: &str =
// r#"{"id": 844424930131970, "label": "Person", "properties": {"name":
// "B"}}::vertex"#;     const KNOWS: &str = r#"{"id": 1125899906842625, "label":
// "KNOWS", "end_id": 844424930131970, "start_id": 844424930131969,
// "properties": {}}::edge"#;

//     fn parse(s: &str) -> AgType {
//         s.parse().unwrap_or_else(|e| panic!("{s}: {e}"))
//     }

//     /// Return the byte offset and kind of the error from parsing `s`.
//     fn parse_err(s: &str) -> (usize, CypherError) {
//         let crate::AgError(AgErrorImpl::Parse(e)) =
//             s.parse::<AgType>().unwrap_err()
//         else {
//             panic!("{s}: not a parse error")
//         };
//         (e.offset, e.kind)
//     }

//     #[test]
//     fn scalars() {
//         assert!(matches!(parse("null"), AgType::Null));
//         assert!(matches!(parse("true"), AgType::Bool(true)));
//         assert!(matches!(parse("false"), AgType::Bool(false)));
//         assert!(matches!(parse("-42"), AgType::Integer(-42)));
//         assert!(matches!(parse("42::integer"), AgType::Integer(42)));
//         assert!(matches!(parse(" 1 "), AgType::Integer(1)));
//     }

//     #[test]
//     fn floats() {
//         assert!(matches!(parse("1.0"), AgType::Float(f) if f == 1.0));
//         assert!(matches!(parse("1e3"), AgType::Float(f) if f == 1000.0));
//         assert!(matches!(parse("2::float"), AgType::Float(f) if f == 2.0));
//         assert!(matches!(parse("NaN"), AgType::Float(f) if f.is_nan()));
//         assert!(
//             matches!(parse("Infinity"), AgType::Float(f) if f ==
// f64::INFINITY)         );
//         assert!(
//             matches!(parse("-Infinity"), AgType::Float(f) if f ==
// f64::NEG_INFINITY)         );
//     }

//     #[test]
//     fn numeric() {
//         let AgType::Numeric(n) = parse("1.10::numeric") else { panic!() };
//         assert_eq!(n, BigDecimal::from_str("1.10").unwrap());
//         assert_eq!(parse_err("NaN::numeric"), (0,
// CypherError::InvalidNumeric));     }

//     #[test]
//     fn nested() {
//         let AgType::Map(m) =
//             parse(r#"{"a": [1, "x", {}], "b": { }, "c": [ ]}"#)
//         else {
//             panic!()
//         };
//         let AgType::List(a) = &m["a"] else { panic!() };
//         assert!(matches!(
//             a[..],
//             [AgType::Integer(1), AgType::String(_), AgType::Map(_)]
//         ));
//         assert!(matches!(&m["b"], AgType::Map(b) if b.is_empty()));
//         assert!(matches!(&m["c"], AgType::List(c) if c.is_empty()));
//     }

//     #[test]
//     fn vertex() {
//         let AgType::Vertex(v) = parse(A) else { panic!() };
//         assert_eq!(v.graph_id(), GraphId(844424930131969));
//         assert_eq!(v.label(), "Person");
//         assert!(matches!(v.properties()["age"], AgType::Integer(29)));
//     }

//     #[test]
//     fn edge() {
//         let AgType::Edge(e) = parse(KNOWS) else { panic!() };
//         assert_eq!(e.label(), "KNOWS");
//         assert_eq!(e.start_id(), GraphId(844424930131969));
//         assert_eq!(e.end_id(), GraphId(844424930131970));
//         assert!(e.properties().is_empty());
//     }

//     #[test]
//     fn path() {
//         let AgType::Path(p) = parse(&format!("[{A}, {KNOWS}, {B}]::path"))
//         else {
//             panic!()
//         };
//         assert_eq!(p.start().label(), "Person");
//         assert_eq!(p.segments().len(), 1);
//         assert_eq!(p.segments()[0].edge().label(), "KNOWS");
//         assert_eq!(
//             p.segments()[0].vertex().graph_id(),
//             GraphId(844424930131970)
//         );

//         let AgType::Path(p) = parse(&format!("[{A}]::path")) else { panic!()
// };         assert!(p.segments().is_empty());
//     }

//     #[test]
//     fn invalid_paths() {
//         for s in [
//             "[]::path".to_string(),
//             format!("[{KNOWS}]::path"),
//             format!("[{A}, {KNOWS}]::path"),
//             format!("[{A}, {B}]::path"),
//         ] {
//             assert_eq!(parse_err(&s), (0, CypherError::InvalidPath), "{s}");
//         }
//     }

//     #[test]
//     fn invalid() {
//         use CypherError::*;

//         assert_eq!(
//             parse_err(r#"{"id": 1}::vertex"#),
//             (0, MissingField("label"))
//         );
//         assert_eq!(
//             parse_err(r#"{"id": "1", "label": "L", "properties":
// {}}::vertex"#),             (0, InvalidField("id"))
//         );
//         assert_eq!(
//             parse_err("1::vertex"),
//             (0, UnexpectedAnnotation("vertex".into()))
//         );
//         assert_eq!(
//             parse_err("[1]::edge"),
//             (0, UnexpectedAnnotation("edge".into()))
//         );
//         assert_eq!(parse_err("99999999999999999999"), (0, InvalidInteger));
//         assert_eq!(parse_err(r#"[1, "abc"#), (4, UnterminatedString));
//         assert_eq!(parse_err(r#""a\x""#), (2, InvalidEscape));
//         assert_eq!(parse_err(r#""\ud800""#), (1, InvalidEscape));
//         assert_eq!(parse_err("1 2"), (2, Syntax));
//         assert_eq!(parse_err("[1, ]"), (2, Syntax));
//         assert_eq!(parse_err(""), (0, Syntax));
//     }
// }
