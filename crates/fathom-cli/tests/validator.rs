//! The JSON-Schema-subset validator, shared with `tests/delta_schema.rs`.
//!
//! One validator, two test binaries, included with `#[path]` rather than duplicated: a second copy is a
//! second opinion nobody asked for, and the day a keyword is added to one and not the other is the day a
//! schema test starts passing for the wrong reason. The subset it implements is the subset the checked-in
//! schemas use — see the module doc of `crates/fathom-analyze/src/insight/schema.rs` for why that
//! coincidence is deliberate.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The schemas live at the workspace root, next to the config schema `e0.8` generates.
/// Where `schemas/` is, from a crate two levels below the workspace root.
pub fn schemas_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/fathom-cli is two levels below the workspace root")
        .join("schemas")
}

/// A schema document plus the directory it came from, so `$ref` to a sibling file resolves.
#[derive(Clone)]
pub struct Schema {
    /// The parsed document, so a test can validate against a fragment of it (`$defs/…`) as well as the
    /// whole thing.
    pub document: serde_json::Value,
}

impl Schema {
    pub fn load(name: &str) -> Schema {
        let dir = schemas_dir();
        let text =
            std::fs::read_to_string(dir.join(name)).unwrap_or_else(|err| panic!("{name}: {err}"));
        let _ = dir;
        Schema {
            document: serde_json::from_str(&text)
                .unwrap_or_else(|err| panic!("{name} is not valid JSON: {err}")),
        }
    }
}

/// Open the named schema files so a local `$ref` (`#/$defs/…`) resolves while validating.
///
/// Idempotent: the first call wins, because `SCHEMAS` is a `OnceLock` and every test in the binary sees
/// the same set. Opening a file twice is not an error, and opening more than one is what lets a verb
/// schema `$ref` its way into `common.schema.json`.
pub fn open(names: &[&str]) {
    let _ = SCHEMAS.set(
        names
            .iter()
            .map(|name| {
                let text = std::fs::read_to_string(schemas_dir().join(name))
                    .unwrap_or_else(|err| panic!("{name}: {err}"));
                (
                    (*name).to_owned(),
                    serde_json::from_str::<serde_json::Value>(&text)
                        .unwrap_or_else(|err| panic!("{name} is not valid JSON: {err}")),
                )
            })
            .collect::<BTreeMap<_, _>>(),
    );
}

/// Validate `value` against `schema`, returning every problem found (empty means it conforms).
pub fn validate(schema: &serde_json::Value, value: &serde_json::Value, at: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let Some(object) = schema.as_object() else {
        return problems;
    };

    if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
        let resolved = resolve_ref(reference, schema_root(schema));
        return validate(&resolved, value, at);
    }
    // A `$ref` alongside siblings is legal in 2020-12 but is not what these schemas do; keep the simpler
    // rule and let the schema-well-formedness test complain if it ever changes.
    if let Some(types) = object.get("type")
        && !type_matches(types, value)
    {
        problems.push(format!(
            "{at}: expected type {types}, got {}",
            kind_of(value)
        ));
        return problems;
    }
    if let Some(allowed) = object.get("enum").and_then(Value::as_array)
        && !allowed.iter().any(|candidate| candidate == value)
    {
        problems.push(format!("{at}: {value} is not one of {allowed:?}"));
    }
    if let Some(wanted) = object.get("const")
        && wanted != value
    {
        problems.push(format!("{at}: expected the constant {wanted}, got {value}"));
    }
    if let Some(minimum) = object.get("minimum").and_then(Value::as_f64)
        && let Some(number) = value.as_f64()
        && number < minimum
    {
        problems.push(format!("{at}: {number} is below the minimum {minimum}"));
    }
    if let Some(maximum) = object.get("maximum").and_then(Value::as_f64)
        && let Some(number) = value.as_f64()
        && number > maximum
    {
        problems.push(format!("{at}: {number} is above the maximum {maximum}"));
    }
    if let Some(min) = object.get("minLength").and_then(Value::as_u64)
        && let Some(text) = value.as_str()
        && (text.chars().count() as u64) < min
    {
        problems.push(format!("{at}: shorter than {min} characters"));
    }
    if let Some(max) = object.get("maxLength").and_then(Value::as_u64)
        && let Some(text) = value.as_str()
        && (text.chars().count() as u64) > max
    {
        problems.push(format!("{at}: longer than {max} characters"));
    }
    if let Some(min) = object.get("minItems").and_then(Value::as_u64)
        && let Some(items) = value.as_array()
        && (items.len() as u64) < min
    {
        problems.push(format!("{at}: fewer than {min} items"));
    }
    if let Some(options) = object.get("oneOf").and_then(Value::as_array) {
        let matches = options
            .iter()
            .filter(|option| validate(option, value, at).is_empty())
            .count();
        if matches != 1 {
            problems.push(format!(
                "{at}: matched {matches} of the {} oneOf options",
                options.len()
            ));
        }
    }

    if let Some(array) = value.as_array()
        && let Some(items) = object.get("items")
    {
        for (index, entry) in array.iter().enumerate() {
            problems.extend(validate(items, entry, &format!("{at}[{index}]")));
        }
    }

    if let Some(object_value) = value.as_object() {
        for key in object
            .get("required")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let name = key.as_str().unwrap_or_default();
            if !object_value.contains_key(name) {
                problems.push(format!("{at}: required property {name:?} is missing"));
            }
        }
        let properties = object.get("properties").and_then(Value::as_object);
        let additional = object.get("additionalProperties");
        for (key, entry) in object_value {
            let path = format!("{at}.{key}");
            if let Some(properties) = properties
                && let Some(sub) = properties.get(key)
            {
                problems.extend(validate(sub, entry, &path));
                continue;
            }
            match additional {
                Some(Value::Bool(false)) => {
                    problems.push(format!(
                        "{path}: not in the schema, and additionalProperties is false"
                    ));
                }
                Some(sub) if !sub.is_boolean() => {
                    problems.extend(validate(sub, entry, &path));
                }
                _ => {}
            }
        }
    }
    problems
}

/// Where the current schema came from, so a `$ref` to a sibling file can be found. Threaded through by a
/// side channel rather than a parameter because only `$ref` needs it.
fn schema_root(_schema: &serde_json::Value) -> PathBuf {
    schemas_dir()
}

/// `{"$ref": "common.schema.json#/$defs/finding"}` — a sibling file and a JSON pointer into it.
fn resolve_ref(reference: &str, root: PathBuf) -> serde_json::Value {
    let (file, pointer) = match reference.split_once('#') {
        Some((file, pointer)) => (file, pointer),
        None => (reference, ""),
    };
    let document: serde_json::Value = if file.is_empty() {
        // A local `$ref` is resolved by the caller's own document; the only local refs used are
        // `#/$defs/...` inside the file being validated, whose root the validator walks anyway.
        return local_ref(pointer);
    } else {
        let path = root.join(file);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        serde_json::from_str(&text).expect("the schema file is valid JSON")
    };
    follow(pointer, &document)
}

/// The document the current `$ref` is relative to. The validator is called with one file open at a time;
/// `SCHEMAS` holds them all so a cross-file ref resolves without re-reading from disk mid-walk.
pub static SCHEMAS: std::sync::OnceLock<BTreeMap<String, serde_json::Value>> =
    std::sync::OnceLock::new();

fn local_ref(pointer: &str) -> serde_json::Value {
    let schemas = SCHEMAS
        .get()
        .expect("a schema was loaded before a local $ref");
    // A local `$ref` is looked up in every open schema; the keys are unique across them in practice, and a
    // miss is a hard failure rather than a silent pass.
    for document in schemas.values() {
        let found = follow(pointer, document);
        if !found.is_null() {
            return found;
        }
    }
    panic!("local $ref {pointer} resolved in none of the loaded schemas");
}

fn follow(pointer: &str, document: &serde_json::Value) -> serde_json::Value {
    let mut current = document.clone();
    for segment in pointer
        .trim_start_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
    {
        let key = segment.replace("~1", "/").replace("~0", "~");
        current = match current {
            serde_json::Value::Object(map) => map.get(&key).cloned().unwrap_or_default(),
            _ => return serde_json::Value::Null,
        };
    }
    current
}

pub fn type_matches(types: &serde_json::Value, value: &serde_json::Value) -> bool {
    let candidates: Vec<&str> = match types {
        serde_json::Value::String(name) => vec![name.as_str()],
        serde_json::Value::Array(names) => names.iter().filter_map(Value::as_str).collect(),
        _ => return true,
    };
    candidates.iter().any(|name| match *name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => true,
    })
}

pub fn kind_of(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// The value type the tests build payloads with.
pub use serde_json::Value;

/// The string keywords this validator understands beyond the structural ones, so a schema can assert a
/// shape without pretending the validator checks something it does not: `pattern` is carried for real
/// validators and *deliberately not enforced here*, and that sentence is the honest version of a
/// keyword that would otherwise silently pass everything.
pub const ENFORCED_KEYWORDS: [&str; 13] = [
    "$ref",
    "additionalProperties",
    "const",
    "enum",
    "items",
    "maximum",
    "minimum",
    "oneOf",
    "properties",
    "required",
    "type",
    "maxLength",
    "minLength",
];
