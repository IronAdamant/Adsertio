//! Tool definitions shared by the MCP server and the CLI.
//!
//! One table describes every tool: its name, group, parameters, and handler.
//! The JSON schema, the argument checks, and the CLI flags all come from it,
//! so they cannot drift apart. Unknown arguments are errors, never ignored.

use crate::json::Value;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Str,
    UInt,
    Bool,
    StrList,
    /// An array of objects. The tool checks each object's fields.
    ObjList,
}

pub struct Param {
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
    pub doc: &'static str,
    /// Allowed values for a `Str` parameter, or for each item of a
    /// `StrList`; empty means free text.
    pub choices: &'static [&'static str],
    /// The fields of each object in an `ObjList`; empty for other kinds.
    pub fields: &'static [Param],
}

pub struct Tool {
    pub name: &'static str,
    pub group: &'static str,
    pub description: &'static str,
    pub params: &'static [Param],
    pub run: fn(&Args) -> Result<Value, String>,
}

/// Arguments that passed `Tool::validate`.
pub struct Args {
    pairs: Vec<(String, Value)>,
}

/// Longest single string argument, in bytes.
pub const MAX_STRING: usize = 20_000;

/// The range `limit` actually accepts. Schema minimum and the tool agree.
pub const LIMIT_RANGE: &str =
    "limit must be a whole number from 1 to 500. Omit limit to return every claim.";
/// Largest `limit`.
pub const MAX_PAGE: u64 = 500;
/// Most items in a list argument.
pub const MAX_LIST: usize = 50;

impl Args {
    pub fn str(&self, name: &str) -> Option<&str> {
        self.get(name).and_then(Value::as_str)
    }

    pub fn uint(&self, name: &str) -> Option<u64> {
        self.get(name).and_then(Value::as_u64)
    }

    pub fn bool(&self, name: &str) -> Option<bool> {
        self.get(name).and_then(Value::as_bool)
    }

    pub fn list(&self, name: &str) -> Vec<&str> {
        self.get(name)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    }

    /// The objects of an `ObjList` argument. Empty when it was not passed.
    pub fn objects(&self, name: &str) -> &[Value] {
        self.get(name).and_then(Value::as_array).unwrap_or_default()
    }

    /// `true` when the argument was passed (and is not null).
    pub fn has(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    fn get(&self, name: &str) -> Option<&Value> {
        self.pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
}

impl Tool {
    /// The JSON schema for `inputSchema`. Kept small: every turn of an agent
    /// reads it again.
    pub fn schema(&self) -> Value {
        object_schema(self.params, true)
    }

    /// Check `args` against the parameter table. Unknown fields, wrong types,
    /// missing required fields, and values outside `choices` are all errors.
    pub fn validate(&self, args: &Value) -> Result<Args, String> {
        validate_fields(self.name, self.params, args)
    }
}

/// `{"type":"object","properties":...}` for `params`. Item schemas
/// (`describe` false) carry types and `required` but no descriptions: the
/// parent parameter describes the item once.
fn object_schema(params: &[Param], describe: bool) -> Value {
    let props: Vec<(String, Value)> = params
        .iter()
        .map(|p| (p.name.to_string(), param_schema(p, describe)))
        .collect();
    let required: Vec<Value> = params
        .iter()
        .filter(|p| p.required)
        .map(|p| Value::str(p.name))
        .collect();
    let mut s = vec![
        ("type", Value::str("object")),
        ("properties", Value::Object(props)),
    ];
    // Items leave it out to stay small; the tool still rejects an unknown
    // field in an item.
    if describe {
        s.push(("additionalProperties", Value::Bool(false)));
    }
    if !required.is_empty() {
        s.push(("required", Value::Array(required)));
    }
    Value::obj(s)
}

fn param_schema(p: &Param, describe: bool) -> Value {
    let kind = match p.kind {
        Kind::Str => "string",
        Kind::UInt => "integer",
        Kind::Bool => "boolean",
        Kind::StrList | Kind::ObjList => "array",
    };
    let enumerated = || {
        (
            "enum",
            Value::Array(p.choices.iter().map(|c| Value::str(*c)).collect()),
        )
    };
    let mut f = vec![("type", Value::str(kind))];
    match p.kind {
        Kind::StrList => {
            let mut item = vec![("type", Value::str("string"))];
            if !p.choices.is_empty() {
                item.push(enumerated());
            }
            f.push(("items", Value::obj(item)));
        }
        Kind::ObjList => {
            f.push(("items", object_schema(p.fields, false)));
            f.push(("maxItems", Value::uint(MAX_LIST as u64)));
        }
        // `limit` pages a list, so 0 is not a page. Other counts (offset)
        // still start at 0.
        Kind::UInt if p.name == "limit" => {
            f.push(("minimum", Value::int(1)));
            f.push(("maximum", Value::uint(MAX_PAGE)));
        }
        Kind::UInt => f.push(("minimum", Value::int(0))),
        Kind::Str if !p.choices.is_empty() => f.push(enumerated()),
        Kind::Str | Kind::Bool => {}
    }
    if describe && !p.doc.is_empty() {
        f.push(("description", Value::str(p.doc)));
    }
    Value::obj(f)
}

/// Check `args` against `params`, the way a tool checks its arguments. Used
/// for the tool itself and for each object in an `ObjList` argument. `label`
/// names the tool or the item in the error.
pub fn validate_fields(label: &str, params: &[Param], args: &Value) -> Result<Args, String> {
    let pairs = match args {
        Value::Object(p) => p.clone(),
        Value::Null => Vec::new(),
        other => {
            return Err(format!(
                "arguments must be an object, got {}",
                other.type_name()
            ))
        }
    };
    let unknown: Vec<&str> = pairs
        .iter()
        .map(|(k, _)| k.as_str())
        .filter(|k| !params.iter().any(|p| p.name == *k))
        .collect();
    if !unknown.is_empty() {
        let valid: Vec<&str> = params.iter().map(|p| p.name).collect();
        return Err(format!(
            "unknown argument(s) for {label}: {}. Valid: {}",
            unknown.join(", "),
            valid.join(", ")
        ));
    }
    for p in params {
        let v = pairs.iter().find(|(k, _)| k == p.name).map(|(_, v)| v);
        match v {
            None | Some(Value::Null) if p.required => {
                return Err(format!("missing required argument: {}", p.name))
            }
            None | Some(Value::Null) => {}
            Some(v) => check_value(p, v)?,
        }
    }
    let pairs = pairs
        .into_iter()
        .filter(|(_, v)| *v != Value::Null)
        .collect();
    Ok(Args { pairs })
}

fn check_value(p: &Param, v: &Value) -> Result<(), String> {
    let bad = |want: &str| Err(format!("{} must be {want}, got {}", p.name, v.type_name()));
    match (p.kind, v) {
        (Kind::Str, Value::String(s)) => {
            if s.len() > MAX_STRING {
                return Err(format!("{} is longer than {MAX_STRING} bytes", p.name));
            }
            if !p.choices.is_empty() && !p.choices.contains(&s.as_str()) {
                return Err(format!(
                    "{} must be one of: {}",
                    p.name,
                    p.choices.join(", ")
                ));
            }
            Ok(())
        }
        (Kind::Str, _) => bad("a string"),
        (Kind::UInt, v) if v.as_u64().is_some() => {
            let n = v.as_u64().unwrap();
            if p.name == "limit" && !(1..=MAX_PAGE).contains(&n) {
                return Err(LIMIT_RANGE.into());
            }
            Ok(())
        }
        (Kind::UInt, _) if p.name == "limit" => Err(LIMIT_RANGE.into()),
        (Kind::UInt, _) => Err(format!(
            "{} must be a whole number from 0 up to 18446744073709551615",
            p.name
        )),
        (Kind::Bool, Value::Bool(_)) => Ok(()),
        (Kind::Bool, _) => bad("a boolean"),
        (Kind::StrList, Value::Array(items)) => {
            if items.len() > MAX_LIST {
                return Err(format!("{} has more than {MAX_LIST} items", p.name));
            }
            for item in items {
                match item {
                    Value::String(s) if s.len() <= MAX_STRING => {}
                    _ => return Err(format!("{} must be an array of strings", p.name)),
                }
            }
            Ok(())
        }
        (Kind::StrList, _) => bad("an array of strings"),
        (Kind::ObjList, Value::Array(items)) => {
            if items.len() > MAX_LIST {
                return Err(format!(
                    "{} has more than {MAX_LIST} items; split it into calls of {MAX_LIST} or fewer",
                    p.name
                ));
            }
            for (n, item) in items.iter().enumerate() {
                if !matches!(item, Value::Object(_)) {
                    return Err(format!(
                        "{} item {} must be an object, got {}",
                        p.name,
                        n + 1,
                        item.type_name()
                    ));
                }
            }
            Ok(())
        }
        (Kind::ObjList, _) => bad("an array of objects"),
    }
}
