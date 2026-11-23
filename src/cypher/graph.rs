use serde::Deserialize;
use serde_json::value::RawValue;

use crate::cypher::{CypherError, CypherValue, Entity, parse};

/// Unique ID assigned to a graph entity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GraphId(pub(crate) i64);

impl GraphId {
    /// Return the ID of the label this entity belongs to.
    pub fn label_id(self) -> u16 {
        (self.0 >> 48) as u16
    }

    /// Return the ID of this entity within its label.
    pub fn entry_id(self) -> u64 {
        self.0 as u64 & 0x0000_FFFF_FFFF_FFFF
    }
}

#[derive(Deserialize)]
struct RawVertex<'a> {
    id: i64,
    label: String,
    #[serde(borrow)]
    properties: &'a RawValue,
}

/// The `vertex` graph entity type.
#[derive(Clone, Debug)]
pub struct Vertex<P> {
    pub(crate) id: GraphId,
    pub(crate) label: String,
    pub(crate) properties: P,
}

impl<P> Vertex<P> {
    /// Return the unique graph ID of this vertex.
    pub fn graph_id(&self) -> GraphId {
        self.id
    }

    /// Return the label this vertex has.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Return the properties of this vertex.
    pub fn properties(&self) -> &P {
        &self.properties
    }
}

impl<P: Entity> CypherValue for Vertex<P> {
    fn parse(value: &str) -> Result<Self, CypherError> {
        let vertex = parse::annotated(value, "vertex")?;
        let raw: RawVertex<'_> = serde_json::from_str(vertex)?;
        let properties = P::parse(&raw.label, raw.properties.get())?;
        Ok(Self { id: GraphId(raw.id), label: raw.label, properties })
    }
}

#[derive(Deserialize)]
struct RawEdge<'a> {
    id: i64,
    start_id: i64,
    end_id: i64,
    label: String,
    #[serde(borrow)]
    properties: &'a RawValue,
}

/// The `edge` entity, which encodes a directed connection between exactly two
/// nodes.
#[derive(Clone, Debug)]
pub struct Edge<P> {
    pub(crate) id: GraphId,
    pub(crate) start_id: GraphId,
    pub(crate) end_id: GraphId,
    pub(crate) label: String,
    pub(crate) properties: P,
}

impl<P> Edge<P> {
    /// Return the unique graph ID of this edge.
    pub fn graph_id(&self) -> GraphId {
        self.id
    }

    /// Return the graph ID of the source node.
    pub fn start_id(&self) -> GraphId {
        self.start_id
    }

    /// Return the graph ID of the target node.
    pub fn end_id(&self) -> GraphId {
        self.end_id
    }

    /// Return the label this edge has.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Return the properties of this edge.
    pub fn properties(&self) -> &P {
        &self.properties
    }
}

impl<P: Entity> CypherValue for Edge<P> {
    fn parse(value: &str) -> Result<Self, CypherError> {
        let edge = parse::annotated(value, "edge")?;
        let raw: RawEdge<'_> = serde_json::from_str(edge)?;
        let properties = P::parse(&raw.label, raw.properties.get())?;
        Ok(Self {
            id: GraphId(raw.id),
            start_id: GraphId(raw.start_id),
            end_id: GraphId(raw.end_id),
            label: raw.label,
            properties,
        })
    }
}

/// Edge, vertex pair in a path.
///
/// The edge connects the previous vertex in the path to this vertex.
#[derive(Clone, Debug)]
pub struct Segment<V, E> {
    pub(crate) edge: Edge<E>,
    pub(crate) vertex: Vertex<V>,
}

impl<V, E> Segment<V, E> {
    /// Return the edge leading to the vertex of this segment.
    pub fn edge(&self) -> &Edge<E> {
        &self.edge
    }

    /// Return the vertex this segment ends on.
    pub fn vertex(&self) -> &Vertex<V> {
        &self.vertex
    }
}

/// A path in a graph is a starting vertex followed by a sequence of segments.
///
/// For a path through several labels, make an enum of a closed set of expected
/// expected `V` and `E`.
#[derive(Clone, Debug)]
pub struct Path<V, E> {
    pub(crate) start: Vertex<V>,
    pub(crate) segments: Vec<Segment<V, E>>,
}

impl<V, E> Path<V, E> {
    /// Return the vertex the path starts on.
    pub fn start(&self) -> &Vertex<V> {
        &self.start
    }

    /// Return the segments following the starting vertex.
    pub fn segments(&self) -> &[Segment<V, E>] {
        &self.segments
    }
}

impl<V: Entity, E: Entity> CypherValue for Path<V, E> {
    fn parse(value: &str) -> Result<Self, CypherError> {
        let items = parse::list(parse::annotated(value, "path")?)?;
        let Some((start, rest)) = items.split_first() else {
            return Err(CypherError::InvalidPath);
        };
        if rest.len() % 2 != 0 {
            return Err(CypherError::InvalidPath);
        }
        let start = path_item(start)?;
        let segments = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|s| {
                Ok(Segment { edge: path_item(s[0])?, vertex: path_item(s[1])? })
            })
            .collect::<Result<_, CypherError>>()?;
        Ok(Self { start, segments })
    }
}

/// Parse an element of a path, where an annotation in the wrong position means
/// the path doesn't alternate vertex, edge, vertex.
fn path_item<T: CypherValue>(item: &str) -> Result<T, CypherError> {
    T::parse(item).map_err(|e| match e {
        CypherError::UnexpectedAnnotation(_)
        | CypherError::MissingAnnotation(_) => CypherError::InvalidPath,
        e => e,
    })
}

impl<V, E> IntoIterator for Path<V, E> {
    type IntoIter = std::vec::IntoIter<Segment<V, E>>;
    type Item = Segment<V, E>;

    fn into_iter(self) -> Self::IntoIter {
        self.segments.into_iter()
    }
}

impl<'a, V, E> IntoIterator for &'a Path<V, E> {
    type IntoIter = std::slice::Iter<'a, Segment<V, E>>;
    type Item = &'a Segment<V, E>;

    fn into_iter(self) -> Self::IntoIter {
        self.segments.iter()
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::*;
    use crate::{AgError, AgErrorImpl};

    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    struct Person {
        name: String,
        age: Option<i64>,
    }

    impl Entity for Person {
        fn parse(label: &str, properties: &str) -> Result<Self, AgError> {
            if label != "Person" {
                return Err(serde::de::Error::unknown_variant(
                    label,
                    &["Person"],
                ));
            }
            serde_json::from_str(properties).map_err(AgError::new)
        }

        fn to_parameter_map(&self) -> Result<String, AgError> {
            serde_json::to_string(self).map_err(AgError::new)
        }
    }

    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    struct Knows {}

    impl Entity for Knows {
        fn parse(label: &str, properties: &str) -> Result<Self, AgError> {
            if label != "KNOWS" {
                return Err(serde::de::Error::unknown_variant(
                    label,
                    &["KNOWS"],
                ));
            }
            serde_json::from_str(properties).map_err(AgError::new)
        }

        fn to_parameter_map(&self) -> Result<String, AgError> {
            serde_json::to_string(self).map_err(AgError::new)
        }
    }

    const A: &str = r#"{"id": 844424930131969, "label": "Person", "properties": {"age": 29, "name": "A::edge"}}::vertex"#;
    const B: &str = r#"{"id": 844424930131970, "label": "Person", "properties": {"name": "B"}}::vertex"#;
    const KNOWS: &str = r#"{"id": 1125899906842625, "label": "KNOWS", "end_id": 844424930131970, "start_id": 844424930131969, "properties": {}}::edge"#;

    #[test]
    fn vertex() {
        let v = Vertex::<Person>::parse(A).unwrap();
        assert_eq!(v.graph_id(), GraphId(844424930131969));
        assert_eq!(v.label(), "Person");
        assert_eq!(
            v.properties(),
            &Person { name: "A::edge".into(), age: Some(29) }
        );
    }

    #[test]
    fn edge() {
        let e = Edge::<Knows>::parse(KNOWS).unwrap();
        assert_eq!(e.graph_id(), GraphId(1125899906842625));
        assert_eq!(e.label(), "KNOWS");
        assert_eq!(e.start_id(), GraphId(844424930131969));
        assert_eq!(e.end_id(), GraphId(844424930131970));
        assert_eq!(e.properties(), &Knows {});
    }

    #[test]
    fn path() {
        let p =
            Path::<Person, Knows>::parse(&format!("[{A}, {KNOWS}, {B}]::path"))
                .unwrap();
        assert_eq!(p.start().graph_id(), GraphId(844424930131969));
        assert_eq!(p.segments().len(), 1);
        assert_eq!(p.segments()[0].edge().label(), "KNOWS");
        assert_eq!(
            p.segments()[0].vertex().graph_id(),
            GraphId(844424930131970)
        );

        let p = Path::<Person, Knows>::parse(&format!("[{A}]::path")).unwrap();
        assert!(p.segments().is_empty());
    }

    #[test]
    fn invalid_paths() {
        for s in [
            "[]::path".to_string(),
            format!("[{KNOWS}]::path"),
            format!("[{A}, {KNOWS}]::path"),
            format!("[{A}, {B}]::path"),
            format!("[{A}, {KNOWS}, {KNOWS}]::path"),
        ] {
            assert!(
                matches!(
                    Path::<Person, Knows>::parse(&s),
                    Err(CypherError::InvalidPath)
                ),
                "{s}"
            );
        }
    }

    #[test]
    fn wrong_annotation() {
        assert!(matches!(
            Vertex::<Person>::parse(KNOWS),
            Err(CypherError::UnexpectedAnnotation(a)) if a == "edge"
        ));
        assert!(matches!(
            Edge::<Knows>::parse(A),
            Err(CypherError::UnexpectedAnnotation(a)) if a == "vertex"
        ));
        assert!(matches!(
            Path::<Person, Knows>::parse(&format!("[{A}]")),
            Err(CypherError::MissingAnnotation("path"))
        ));
    }

    #[test]
    fn wrong_label() {
        let Err(CypherError::Serde(e)) = Vertex::<Knows>::parse(A) else {
            panic!("expected a label error");
        };
        let e = e.downcast_ref::<AgError>().unwrap();
        assert!(matches!(
            &e.0,
            AgErrorImpl::UnknownLabel { found, .. } if found == "Person"
        ));
    }

    #[test]
    fn missing_field() {
        assert!(matches!(
            Vertex::<Person>::parse(r#"{"id": 1}::vertex"#),
            Err(CypherError::Json(_))
        ));
    }
}
