//! A small JSON value that keeps key order.
//!
//! The parser is strict and bounded: nesting deeper than `MAX_DEPTH` is an
//! error (never a stack overflow), duplicate object keys are an error, control
//! characters in strings are an error, and lone surrogates are an error.
//! Callers hand it `&str`, so invalid UTF-8 is rejected before it gets here.

use std::fmt;

/// Deepest array/object nesting the parser accepts.
pub const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    /// The number as written, already checked against the JSON grammar.
    Number(String),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn str(s: impl Into<String>) -> Value {
        Value::String(s.into())
    }

    pub fn int(n: i64) -> Value {
        Value::Number(n.to_string())
    }

    pub fn uint(n: u64) -> Value {
        Value::Number(n.to_string())
    }

    pub fn obj(pairs: Vec<(&str, Value)>) -> Value {
        Value::Object(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// An integer that fits `u64`; `None` for fractions, exponents, negatives.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Number(n) if n.bytes().all(|b| b.is_ascii_digit()) => n.parse().ok(),
            _ => None,
        }
    }

    /// An integer that fits `i64`. Fractions and exponents are `None`: a
    /// timestamp or an id written as `1.0` is not treated as `1`.
    pub fn as_i64(&self) -> Option<i64> {
        let Value::Number(n) = self else {
            return None;
        };
        let digits = n.strip_prefix('-').unwrap_or(n);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        n.parse().ok()
    }

    /// A finite JSON number. `None` for non-numbers and for infinities.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => n.parse::<f64>().ok().filter(|n| n.is_finite()),
            _ => None,
        }
    }

    /// Replace `key`, or append it. No effect unless `self` is an object.
    pub fn insert(&mut self, key: &str, value: Value) {
        let Value::Object(pairs) = self else {
            return;
        };
        if let Some((_, slot)) = pairs.iter_mut().find(|(k, _)| k == key) {
            *slot = value;
        } else {
            pairs.push((key.to_string(), value));
        }
    }

    /// Remove `key` and return its value.
    pub fn take(&mut self, key: &str) -> Option<Value> {
        let Value::Object(pairs) = self else {
            return None;
        };
        let index = pairs.iter().position(|(k, _)| k == key)?;
        Some(pairs.remove(index).1)
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "a boolean",
            Value::Number(_) => "a number",
            Value::String(_) => "a string",
            Value::Array(_) => "an array",
            Value::Object(_) => "an object",
        }
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(true) => out.push_str("true"),
            Value::Bool(false) => out.push_str("false"),
            Value::Number(n) => out.push_str(n),
            Value::String(s) => write_string(s, out),
            Value::Array(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write(out);
                }
                out.push(']');
            }
            Value::Object(pairs) => {
                out.push('{');
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_string(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

pub fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[derive(Debug, Clone, PartialEq)]
pub struct JsonError {
    pub message: String,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

pub fn parse(input: &str) -> Result<Value, JsonError> {
    let mut p = Parser {
        s: input.as_bytes(),
        i: 0,
    };
    p.skip_ws();
    let v = p.value(0)?;
    p.skip_ws();
    if p.i != p.s.len() {
        return Err(p.err("trailing data after the JSON value"));
    }
    Ok(v)
}

pub(crate) enum Kind {
    Null,
    Bool,
    Number,
    String,
    Array,
    Object,
}

/// A JSON number, kept as written so integers and fractions stay distinct.
pub(crate) struct Number {
    text: String,
}

impl Number {
    pub(crate) fn as_i64(&self) -> Option<i64> {
        Value::Number(self.text.clone()).as_i64()
    }

    pub(crate) fn as_value(&self) -> Value {
        Value::Number(self.text.clone())
    }
}

pub(crate) struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    pub(crate) fn new(input: &str) -> Parser<'_> {
        Parser {
            s: input.as_bytes(),
            i: 0,
        }
    }

    pub(crate) fn finish(&mut self) -> Result<(), JsonError> {
        self.skip_ws();
        if self.i != self.s.len() {
            Err(self.err("trailing data after the JSON value"))
        } else {
            Ok(())
        }
    }

    pub(crate) fn kind(&mut self) -> Result<Kind, JsonError> {
        self.skip_ws();
        match self.peek() {
            Some(b'n') => Ok(Kind::Null),
            Some(b't' | b'f') => Ok(Kind::Bool),
            Some(b'"') => Ok(Kind::String),
            Some(b'[') => Ok(Kind::Array),
            Some(b'{') => Ok(Kind::Object),
            Some(b'-' | b'0'..=b'9') => Ok(Kind::Number),
            Some(_) => Err(self.err("unexpected character")),
            None => Err(self.err("unexpected end of input")),
        }
    }

    pub(crate) fn take_value(&mut self, depth: usize) -> Result<Value, JsonError> {
        self.skip_ws();
        self.value(depth)
    }

    pub(crate) fn take_string(&mut self) -> Result<String, JsonError> {
        self.skip_ws();
        if self.peek() != Some(b'"') {
            return Err(self.err("expected a string"));
        }
        self.string()
    }

    pub(crate) fn take_bool(&mut self) -> Result<bool, JsonError> {
        self.skip_ws();
        match self.peek() {
            Some(b't') => self.eat("true").map(|_| true),
            Some(b'f') => self.eat("false").map(|_| false),
            _ => Err(self.err("expected a boolean")),
        }
    }

    pub(crate) fn take_number(&mut self) -> Result<Number, JsonError> {
        self.skip_ws();
        let value = self.number()?;
        match value {
            Value::Number(text) => Ok(Number { text }),
            _ => Err(self.err("expected a number")),
        }
    }

    pub(crate) fn skip_value(&mut self, depth: usize) -> Result<(), JsonError> {
        self.skip_ws();
        match self.peek() {
            None => Err(self.err("unexpected end of input")),
            Some(b'n') => self.eat("null"),
            Some(b't') => self.eat("true"),
            Some(b'f') => self.eat("false"),
            Some(b'"') => self.skip_string(),
            Some(b'[') => self.skip_array(depth),
            Some(b'{') => self.skip_object(depth),
            Some(b'-' | b'0'..=b'9') => self.number().map(|_| ()),
            Some(_) => Err(self.err("unexpected character")),
        }
    }

    /// Call `on_key` once per field. The callback must consume exactly one value.
    pub(crate) fn with_object<E: From<JsonError>>(
        &mut self,
        depth: usize,
        mut on_key: impl FnMut(&mut Self, &str, usize) -> Result<(), E>,
    ) -> Result<(), E> {
        let depth = self.enter(depth)?;
        self.skip_ws();
        if self.peek() != Some(b'{') {
            return Err(self.err("expected an object").into());
        }
        self.i += 1;
        let mut seen: Vec<String> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(());
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected a string key").into());
            }
            let key = self.string()?;
            if seen.iter().any(|seen| seen == &key) {
                return Err(self.err(&format!("duplicate key \"{key}\"")).into());
            }
            seen.push(key);
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(self.err("expected ':'").into());
            }
            self.i += 1;
            self.skip_ws();
            let key = seen.last().unwrap().clone();
            on_key(self, &key, depth)?;
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(());
                }
                _ => return Err(self.err("expected ',' or '}'").into()),
            }
        }
    }

    /// Call `on_item` once per element. The callback must consume that value.
    pub(crate) fn with_array<E: From<JsonError>>(
        &mut self,
        depth: usize,
        mut on_item: impl FnMut(&mut Self, usize) -> Result<(), E>,
    ) -> Result<(), E> {
        let _child = self.enter(depth)?;
        self.skip_ws();
        if self.peek() != Some(b'[') {
            return Err(self.err("expected an array").into());
        }
        self.i += 1;
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(());
        }
        let mut index = 0usize;
        loop {
            self.skip_ws();
            on_item(self, index)?;
            index += 1;
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(());
                }
                _ => return Err(self.err("expected ',' or ']'").into()),
            }
        }
    }

    fn skip_array(&mut self, depth: usize) -> Result<(), JsonError> {
        self.with_array(depth, |parser, _| parser.skip_value(depth + 1))
    }

    fn skip_object(&mut self, depth: usize) -> Result<(), JsonError> {
        self.with_object(depth, |parser, _, child| parser.skip_value(child))
    }

    fn skip_string(&mut self) -> Result<(), JsonError> {
        self.string_into(None)
    }

    fn err(&self, what: &str) -> JsonError {
        JsonError {
            message: format!("{what} at byte {}", self.i),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    pub(crate) fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn eat(&mut self, lit: &str) -> Result<(), JsonError> {
        if self.s[self.i..].starts_with(lit.as_bytes()) {
            self.i += lit.len();
            Ok(())
        } else {
            Err(self.err("invalid literal"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, JsonError> {
        match self.peek() {
            None => Err(self.err("unexpected end of input")),
            Some(b'n') => self.eat("null").map(|_| Value::Null),
            Some(b't') => self.eat("true").map(|_| Value::Bool(true)),
            Some(b'f') => self.eat("false").map(|_| Value::Bool(false)),
            Some(b'"') => self.string().map(Value::String),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.err("unexpected character")),
        }
    }

    fn enter(&self, depth: usize) -> Result<usize, JsonError> {
        if depth >= MAX_DEPTH {
            Err(self.err(&format!("nesting deeper than {MAX_DEPTH} levels")))
        } else {
            Ok(depth + 1)
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, JsonError> {
        let depth = self.enter(depth)?;
        self.i += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value(depth)?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(self.err("expected ',' or ']'")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, JsonError> {
        let depth = self.enter(depth)?;
        self.i += 1;
        let mut pairs: Vec<(String, Value)> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Value::Object(pairs));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected a string key"));
            }
            let key = self.string()?;
            if pairs.iter().any(|(k, _)| *k == key) {
                return Err(self.err(&format!("duplicate key \"{key}\"")));
            }
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(self.err("expected ':'"));
            }
            self.i += 1;
            self.skip_ws();
            let v = self.value(depth)?;
            pairs.push((key, v));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Value::Object(pairs));
                }
                _ => return Err(self.err("expected ',' or '}'")),
            }
        }
    }

    fn number(&mut self) -> Result<Value, JsonError> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        match self.peek() {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(self.err("invalid number")),
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("invalid number"));
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("invalid number"));
            }
            self.digits();
        }
        // The slice is ASCII digits and punctuation only.
        let text = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("0");
        Ok(Value::Number(text.to_string()))
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.i += 1;
        }
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        if self.i + 4 > self.s.len() {
            return Err(self.err("short \\u escape"));
        }
        let mut n = 0u32;
        for _ in 0..4 {
            let d = (self.s[self.i] as char)
                .to_digit(16)
                .ok_or_else(|| self.err("bad \\u escape"))?;
            n = n * 16 + d;
            self.i += 1;
        }
        Ok(n)
    }

    fn string(&mut self) -> Result<String, JsonError> {
        let mut out = Vec::new();
        self.string_into(Some(&mut out))?;
        // Input came from a &str and escapes were encoded as UTF-8.
        String::from_utf8(out).map_err(|_| self.err("invalid UTF-8 in string"))
    }

    fn string_into(&mut self, mut out: Option<&mut Vec<u8>>) -> Result<(), JsonError> {
        self.i += 1;
        loop {
            let b = self.peek().ok_or_else(|| self.err("unterminated string"))?;
            self.i += 1;
            match b {
                b'"' => return Ok(()),
                b'\\' => {
                    let e = self.peek().ok_or_else(|| self.err("unterminated escape"))?;
                    self.i += 1;
                    let c = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            if (0xD800..0xDC00).contains(&hi) {
                                if self.s[self.i..].starts_with(b"\\u") {
                                    self.i += 2;
                                    let lo = self.hex4()?;
                                    if !(0xDC00..0xE000).contains(&lo) {
                                        return Err(self.err("invalid surrogate pair"));
                                    }
                                    char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00))
                                        .ok_or_else(|| self.err("invalid surrogate pair"))?
                                } else {
                                    return Err(self.err("lone surrogate"));
                                }
                            } else if (0xDC00..0xE000).contains(&hi) {
                                return Err(self.err("lone surrogate"));
                            } else {
                                char::from_u32(hi).ok_or_else(|| self.err("bad \\u escape"))?
                            }
                        }
                        _ => return Err(self.err("invalid escape")),
                    };
                    if let Some(out) = out.as_mut() {
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    }
                }
                0x00..=0x1f => return Err(self.err("control character in string")),
                _ => {
                    if let Some(out) = out.as_mut() {
                        out.push(b);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "json_tests.rs"]
mod tests;
