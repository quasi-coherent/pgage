use pgage::Entity;
use pgage::agtype::AgType;
use pgage::cypher::{Edge, Entity as _, Map, Path, Scalar, Vertex};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgArgumentBuffer;
use sqlx::{
    AssertSqlSafe, Connection, Encode, Executor, PgConnection, Postgres, Type,
};
use std::collections::HashMap;

#[derive(Debug, Deserialize, Entity, PartialEq, Serialize)]
struct Person {
    name: String,
    age: Option<i64>,
}

#[derive(Debug, Deserialize, Entity, PartialEq, Serialize)]
#[pgage(label = "KNOWS")]
struct Knows {}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct Dog {
    name: String,
}

#[derive(Debug, Deserialize, Entity, PartialEq, Serialize)]
enum Node {
    Person(Person),
    #[pgage(label = "Dawg")]
    Dog(Dog),
}

fn encoded<T: for<'q> Encode<'q, Postgres>>(value: &T) -> String {
    let mut buf = PgArgumentBuffer::default();
    let _ = value.encode_by_ref(&mut buf).unwrap();
    let [pgage::private::AGTYPE_VERSION, rest @ ..] = &buf[..] else {
        panic!("missing version byte");
    };
    String::from_utf8(rest.to_vec()).unwrap()
}

#[test]
fn derive_struct() {
    let p = Person::parse("Person", r#"{"name": "A", "age": 29}"#).unwrap();
    assert_eq!(p, Person { name: "A".into(), age: Some(29) });
    assert!(Person::parse("Dog", r#"{"name": "A"}"#).is_err());
    assert!(Person::parse("Person", r#"{"age": 29}"#).is_err());
    assert_eq!(p.to_parameter_map().unwrap(), r#"{"name":"A","age":29}"#);
}

#[test]
fn derive_label() {
    assert_eq!(Knows::parse("KNOWS", "{}").unwrap(), Knows {});
    assert!(Knows::parse("Knows", "{}").is_err());
}

#[test]
fn derive_enum() {
    assert_eq!(
        Node::parse("Person", r#"{"name": "A"}"#).unwrap(),
        Node::Person(Person { name: "A".into(), age: None })
    );
    let dog = Node::parse("Dawg", r#"{"name": "Rex"}"#).unwrap();
    assert_eq!(dog, Node::Dog(Dog { name: "Rex".into() }));
    assert!(Node::parse("Dog", r#"{"name": "Rex"}"#).is_err());
    assert_eq!(dog.to_parameter_map().unwrap(), r#"{"name":"Rex"}"#);
}

#[test]
fn derive_encode() {
    assert_eq!(
        Person::type_info(),
        sqlx::postgres::PgTypeInfo::with_name("agtype")
    );
    let p = Person { name: "A".into(), age: None };
    assert_eq!(encoded(&p), r#"{"name":"A","age":null}"#);
    assert_eq!(
        encoded(&Node::Dog(Dog { name: "Rex".into() })),
        r#"{"name":"Rex"}"#
    );
}

async fn graph(name: &str) -> PgConnection {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://postgres:postgres@localhost:5432".into()
    });
    let mut c = PgConnection::connect(&url).await.unwrap();
    c.execute("begin; load 'age'; set search_path = ag_catalog, public;")
        .await
        .unwrap();
    c.execute(AssertSqlSafe(format!("select create_graph('{name}')")))
        .await
        .unwrap();
    c
}

#[tokio::test]
async fn db_vertex() {
    let mut c = graph("db_vertex").await;
    let a = Person { name: "A".into(), age: Some(29) };
    let (v,): (AgType<Vertex<Person>>,) = sqlx::query_as(
        "select * from cypher('db_vertex', $$
            create (p:Person {name: $name, age: $age}) return p
        $$, $1) as (p agtype)",
    )
    .bind(&a)
    .fetch_one(&mut c)
    .await
    .unwrap();
    assert_eq!(v.label(), "Person");
    assert_eq!(v.properties(), &a);

    let (w,): (AgType<Vertex<Person>>,) = sqlx::query_as(
        "select * from cypher('db_vertex', $$
            match (p:Person) return p
        $$) as (p agtype)",
    )
    .fetch_one(&mut c)
    .await
    .unwrap();
    assert_eq!(w.graph_id(), v.graph_id());
    assert_eq!(w.properties(), &a);
}

#[tokio::test]
async fn db_edge_and_path() {
    let mut c = graph("db_edge_and_path").await;
    let (e, p): (AgType<Edge<Knows>>, AgType<Path<Person, Knows>>) =
        sqlx::query_as(
            "select * from cypher('db_edge_and_path', $$
                create path = (a:Person {name: 'A'})-[e:KNOWS]->(b:Person {name: 'B'})
                return e, path
            $$) as (e agtype, p agtype)",
        )
        .fetch_one(&mut c)
        .await
        .unwrap();
    assert_eq!(e.label(), "KNOWS");
    assert_eq!(e.properties(), &Knows {});
    assert_eq!(p.start().properties().name, "A");
    assert_eq!(p.segments().len(), 1);
    let s = &p.segments()[0];
    assert_eq!(s.edge().graph_id(), e.graph_id());
    assert_eq!(s.edge().start_id(), p.start().graph_id());
    assert_eq!(s.edge().end_id(), s.vertex().graph_id());
    assert_eq!(s.vertex().properties().name, "B");
}

#[tokio::test]
async fn db_enum_path() {
    let mut c = graph("db_enum_path").await;
    let (p,): (AgType<Path<Node, Knows>>,) = sqlx::query_as(
        "select * from cypher('db_enum_path', $$
            create path = (:Person {name: 'A'})-[:KNOWS]->(:Dawg {name: 'Rex'})
            return path
        $$) as (p agtype)",
    )
    .fetch_one(&mut c)
    .await
    .unwrap();
    assert_eq!(
        p.start().properties(),
        &Node::Person(Person { name: "A".into(), age: None })
    );
    assert_eq!(
        p.segments()[0].vertex().properties(),
        &Node::Dog(Dog { name: "Rex".into() })
    );
}

#[tokio::test]
async fn db_collect() {
    let mut c = graph("db_collect").await;
    c.execute(
        "select * from cypher('db_collect', $$
            create (:Person {name: 'A'}), (:Person {name: 'B'})
        $$) as (v agtype)",
    )
    .await
    .unwrap();
    let (vs, names): (AgType<Vec<Vertex<Person>>>, AgType<Vec<String>>) =
        sqlx::query_as(
            "select * from cypher('db_collect', $$
                match (p:Person) with p order by p.name
                return collect(p), collect(p.name)
            $$) as (vs agtype, names agtype)",
        )
        .fetch_one(&mut c)
        .await
        .unwrap();
    let vs: Vec<_> =
        vs.v().into_iter().map(|v| v.properties().name.clone()).collect();
    assert_eq!(vs, ["A", "B"]);
    assert_eq!(names.v(), ["A", "B"]);
}

#[tokio::test]
async fn db_scalars() {
    let mut c = graph("db_scalars").await;
    let row: (
        AgType<i64>,
        AgType<u8>,
        AgType<f64>,
        AgType<bool>,
        AgType<String>,
        AgType<Vec<i32>>,
    ) = sqlx::query_as(
        r#"select * from cypher('db_scalars', $$
            return -42, 255, 2.5, true, "a\"é", [1, 2, 3]
        $$) as (a agtype, b agtype, c agtype, d agtype, e agtype, f agtype)"#,
    )
    .fetch_one(&mut c)
    .await
    .unwrap();
    assert_eq!(row.0.v(), -42);
    assert_eq!(row.1.v(), 255);
    assert_eq!(row.2.v(), 2.5);
    assert!(row.3.v());
    assert_eq!(row.4.v(), "a\"é");
    assert_eq!(row.5.v(), [1, 2, 3]);

    let (n,): (Option<AgType<i64>>,) = sqlx::query_as(
        "select * from cypher('db_scalars', $$ return null $$) as (n agtype)",
    )
    .fetch_one(&mut c)
    .await
    .unwrap();
    assert!(n.is_none());
}

#[tokio::test]
async fn db_map() {
    let mut c = graph("db_map").await;
    let params = Map(HashMap::from([
        ("name".to_string(), Scalar::String("A".into())),
        ("age".to_string(), Scalar::Integer(29)),
    ]));
    let (v,): (AgType<Vertex<Person>>,) = sqlx::query_as(
        "select * from cypher('db_map', $$
            create (p:Person {name: $name, age: $age}) return p
        $$, $1) as (p agtype)",
    )
    .bind(&params)
    .fetch_one(&mut c)
    .await
    .unwrap();
    assert_eq!(v.properties(), &Person { name: "A".into(), age: Some(29) });

    let (m,): (AgType<Map>,) = sqlx::query_as(
        "select * from cypher('db_map', $$
            return {i: 1, f: 1.5, s: 'x', b: false, n: null}
        $$) as (m agtype)",
    )
    .fetch_one(&mut c)
    .await
    .unwrap();
    let m = m.v().0;
    assert_eq!(m["i"], Scalar::Integer(1));
    assert_eq!(m["f"], Scalar::Float(1.5));
    assert_eq!(m["s"], Scalar::String("x".into()));
    assert_eq!(m["b"], Scalar::Bool(false));
    assert_eq!(m["n"], Scalar::Null);
}

#[tokio::test]
async fn db_wrong_label() {
    let mut c = graph("db_wrong_label").await;
    let r: Result<(AgType<Vertex<Knows>>,), _> = sqlx::query_as(
        "select * from cypher('db_wrong_label', $$
            create (p:Person {name: 'A'}) return p
        $$) as (p agtype)",
    )
    .fetch_one(&mut c)
    .await;
    assert!(matches!(r, Err(sqlx::Error::ColumnDecode { .. })));
}
