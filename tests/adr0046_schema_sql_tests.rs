//! ADR 0046 item 6: `src/schema.sql` states the shape that the migrations build.
//!
//! The node never runs `src/schema.sql`. Only the migrations build a database,
//! so this file can become false with no failure at start. The test compares
//! the tables, the columns, the indexes and the triggers of the two shapes. An
//! index compares by its table and columns, because a migration can give the
//! same index a different name.

mod common;

use std::collections::BTreeMap;

use rusqlite::Connection;

/// One text value for each table, index and trigger of a database.
type Shape = BTreeMap<String, String>;

/// Lowers the case, collapses white space and removes `IF NOT EXISTS`.
fn normalized(sql: &str) -> String {
    sql.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .replace("if not exists ", "")
}

/// Gives each column of `table`: its name, type, `NOT NULL`, default and key.
fn columns(conn: &Connection, table: &str) -> String {
    let mut stmt = conn
        .prepare(
            "SELECT name, type, \"notnull\", dflt_value, pk \
             FROM pragma_table_info(?1) ORDER BY name",
        )
        .expect("prepare the column query");
    stmt.query_map([table], |row| {
        Ok(format!(
            "{} {} notnull={} default={:?} pk={}",
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, i64>(4)?,
        ))
    })
    .expect("read the columns")
    .collect::<Result<Vec<_>, _>>()
    .expect("decode the columns")
    .join(", ")
}

/// Reads the shape of `conn`. The migration runner owns `schema_migrations`,
/// and `SQLite` owns each `sqlite_` object, so the shape leaves them out.
fn shape(conn: &Connection) -> Shape {
    let mut stmt = conn
        .prepare(
            "SELECT type, name, sql FROM sqlite_master \
             WHERE name NOT LIKE 'sqlite_%' AND name <> 'schema_migrations' AND sql IS NOT NULL",
        )
        .expect("prepare the shape query");
    let objects = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .expect("read the shape")
        .collect::<Result<Vec<_>, _>>()
        .expect("decode the shape");

    objects
        .into_iter()
        .map(|(kind, name, sql)| match kind.as_str() {
            "table" => (format!("table {name}"), columns(conn, &name)),
            "index" => {
                let sql = normalized(&sql);
                let unique = if sql.starts_with("create unique") {
                    "unique "
                } else {
                    ""
                };
                let target = sql.split_once(" on ").map_or("", |(_, target)| target);
                (format!("{unique}index on {target}"), String::new())
            }
            _ => (format!("{kind} {name}"), normalized(&sql)),
        })
        .collect()
}

#[test]
fn schema_sql_states_the_shape_that_the_migrations_build() {
    let migrated = shape(&common::test_db());

    let schema_conn = Connection::open_in_memory().expect("open a database");
    schema_conn
        .execute_batch(include_str!("../src/schema.sql"))
        .expect("apply src/schema.sql");
    let stated = shape(&schema_conn);

    let mut differences = Vec::new();
    for (key, value) in &stated {
        match migrated.get(key) {
            None => differences.push(format!("only in src/schema.sql: {key}")),
            Some(built) if built != value => {
                differences.push(format!(
                    "differs: {key}\n  src/schema.sql: {value}\n  migrations: {built}"
                ));
            }
            Some(_) => {}
        }
    }
    for key in migrated.keys() {
        if !stated.contains_key(key) {
            differences.push(format!("only in the migrations: {key}"));
        }
    }

    assert!(
        differences.is_empty(),
        "ADR 0046 item 6: src/schema.sql differs from the database that the migrations build.\n\
         {}\n\
         Change src/schema.sql to match the migrations. If the migrations are wrong, add a migration.",
        differences.join("\n")
    );
}
