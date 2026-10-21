//! ADR 0044 §4: a field rename or a field removal in a `v1` response is a
//! breaking change, and needs a new path version.
//!
//! `tests/fixtures/v1-schema-fields.json` gives the properties and the enum
//! values of each schema of the document. This test fails when one of them
//! is gone. An added field or value is not a breaking change, so the test
//! permits it. After an addition, add the new name to the fixture.

use serde_json::Value;

fn current_schemas() -> Value {
    let document = serde_json::to_value(stophammer::openapi::primary_document())
        .expect("serialize the document");
    document["components"]["schemas"].clone()
}

fn recorded_schemas() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/v1-schema-fields.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("read the fixture"))
        .expect("parse the fixture")
}

fn names(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("a list of names")
        .iter()
        .map(|name| name.as_str().expect("a name").to_string())
        .collect()
}

#[test]
fn no_recorded_v1_field_or_enum_value_is_removed() {
    let current = current_schemas();
    let recorded = recorded_schemas();
    for (schema, fields) in recorded.as_object().expect("an object of schemas") {
        let now = &current[schema];
        assert!(
            !now.is_null(),
            "ADR 0044 §4: the schema {schema} is gone from the v1 document. \
             A removal needs a new path version"
        );
        if let Some(properties) = fields.get("properties") {
            for property in names(properties) {
                assert!(
                    now["properties"].get(&property).is_some(),
                    "ADR 0044 §4: {schema}.{property} is gone from the v1 document. \
                     A rename or a removal needs a new path version"
                );
            }
        }
        if let Some(values) = fields.get("enum") {
            let now_values: Vec<String> = now["enum"]
                .as_array()
                .map(|v| {
                    v.iter()
                        .map(|x| x.as_str().unwrap_or_default().to_string())
                        .collect()
                })
                .unwrap_or_default();
            for value in names(values) {
                assert!(
                    now_values.contains(&value),
                    "ADR 0044 §4: the value {value} of {schema} is gone from the v1 document. \
                     A removal needs a new path version"
                );
            }
        }
    }
}
