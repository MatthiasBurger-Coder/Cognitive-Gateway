use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PathPart {
    Member(String),
    Index(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Location {
    pub source_id: String,
    pub span: Span,
    pub path: Vec<PathPart>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub code: &'static str,
    pub phase: &'static str,
    pub severity: &'static str,
    pub message: String,
    #[serde(flatten)]
    pub location: Box<Location>,
    pub related: Vec<Location>,
}

impl Diagnostic {
    pub(crate) fn new(
        code: &'static str,
        phase: &'static str,
        location: Location,
        message: impl ToString,
    ) -> Self {
        Self {
            code,
            phase,
            severity: "ERROR",
            message: message.to_string(),
            location: Box::new(location),
            related: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub key: String,
    pub key_span: Span,
    pub span: Span,
    pub value: Node,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub span: Span,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Object(Vec<Member>),
    Array(Vec<Node>),
    String(String),
    Number(String),
    Bool(bool),
    Null,
}

impl Node {
    pub fn member(&self, key: &str) -> Option<&Node> {
        match &self.value {
            Value::Object(members) => members.iter().find(|m| m.key == key).map(|m| &m.value),
            _ => None,
        }
    }

    /// Exact-token JSON rendering; no floating point intermediary.
    pub fn json(&self) -> String {
        match &self.value {
            Value::Object(members) => format!(
                "{{{}}}",
                members
                    .iter()
                    .map(|m| format!("{}:{}", quote(&m.key), m.value.json()))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Value::Array(values) => format!(
                "[{}]",
                values.iter().map(Node::json).collect::<Vec<_>>().join(",")
            ),
            Value::String(value) => quote(value),
            Value::Number(value) => value.clone(),
            Value::Bool(value) => value.to_string(),
            Value::Null => "null".into(),
        }
    }
}

fn quote(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization is infallible")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub source_id: String,
    pub root: Node,
}

impl Document {
    pub fn location(&self, node: &Node, path: Vec<PathPart>) -> Location {
        Location {
            source_id: self.source_id.clone(),
            span: node.span,
            path,
        }
    }
}

/// Fixed implementation budgets: 1 MiB input, 64 nested containers, 100,000 values.
pub const MAX_SOURCE_BYTES: usize = 1_048_576;
pub const MAX_DEPTH: usize = 64;
pub const MAX_NODES: usize = 100_000;

pub fn parse(source_id: &str, bytes: &[u8]) -> Result<Document, Diagnostic> {
    let location = |start, end| Location {
        source_id: source_id.into(),
        span: Span { start, end },
        path: vec![],
    };
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(Diagnostic::new(
            "CGSL_RESOURCE_LIMIT",
            "LIMIT",
            location(0, bytes.len()),
            "source exceeds 1 MiB",
        ));
    }
    let source = std::str::from_utf8(bytes).map_err(|e| {
        Diagnostic::new(
            "SYN_INVALID_ENCODING",
            "LEX",
            location(
                e.valid_up_to(),
                e.valid_up_to() + e.error_len().unwrap_or(bytes.len() - e.valid_up_to()),
            ),
            "invalid UTF-8",
        )
    })?;
    if source.starts_with('\u{feff}') {
        return Err(Diagnostic::new(
            "SYN_INVALID_ENCODING",
            "LEX",
            location(0, 3),
            "BOM is forbidden",
        ));
    }
    let mut parser = Parser {
        source,
        source_id,
        pos: 0,
        nodes: 0,
        path: vec![],
    };
    parser.ws();
    if parser.peek() != Some(b'{') {
        return Err(parser.error("SYN_EXPECTED_TOKEN", "PARSE", "expected root object"));
    }
    let root = parser.value(0)?;
    parser.ws();
    if parser.pos != source.len() {
        return Err(parser.error("SYN_TRAILING_CONTENT", "PARSE", "content after root"));
    }
    Ok(Document {
        source_id: source_id.into(),
        root,
    })
}

struct Parser<'a> {
    source: &'a str,
    source_id: &'a str,
    pos: usize,
    nodes: usize,
    path: Vec<PathPart>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.source.as_bytes().get(self.pos).copied()
    }
    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.pos += 1;
        }
    }
    fn error(&self, code: &'static str, phase: &'static str, message: &str) -> Diagnostic {
        Diagnostic::new(
            code,
            phase,
            Location {
                source_id: self.source_id.into(),
                span: Span {
                    start: self.pos,
                    end: (self.pos + 1).min(self.source.len()),
                },
                path: self.path.clone(),
            },
            message,
        )
    }
    fn take(&mut self, byte: u8) -> Result<(), Diagnostic> {
        self.ws();
        if matches!(self.peek(), Some(b'/' | b'#')) {
            return Err(self.error("SYN_INVALID_TOKEN", "LEX", "comments are forbidden"));
        }
        if self.peek() != Some(byte) {
            return Err(self.error("SYN_EXPECTED_TOKEN", "PARSE", "expected delimiter"));
        }
        self.pos += 1;
        Ok(())
    }
    fn string(&mut self) -> Result<String, Diagnostic> {
        self.take(b'"')?;
        let start = self.pos - 1;
        while let Some(byte) = self.peek() {
            self.pos += 1;
            if byte == b'"' {
                return serde_json::from_str(&self.source[start..self.pos]).map_err(|_| {
                    let mut error = self.error(
                        "SYN_INVALID_TOKEN",
                        "LEX",
                        "invalid string escape or control",
                    );
                    error.location.span = Span {
                        start,
                        end: self.pos,
                    };
                    error
                });
            }
            if byte == b'\\' && self.peek().is_some() {
                self.pos += 1;
            }
        }
        Err(self.error("SYN_EXPECTED_TOKEN", "PARSE", "unterminated string"))
    }
    fn value(&mut self, depth: usize) -> Result<Node, Diagnostic> {
        self.ws();
        self.nodes += 1;
        if depth > MAX_DEPTH || self.nodes > MAX_NODES {
            return Err(self.error("CGSL_RESOURCE_LIMIT", "LIMIT", "AST budget exceeded"));
        }
        let start = self.pos;
        let value = match self.peek() {
            Some(b'{') => {
                self.pos += 1;
                self.ws();
                let mut members = Vec::new();
                let mut keys = BTreeMap::new();
                if self.peek() != Some(b'}') {
                    loop {
                        self.ws();
                        let key_start = self.pos;
                        let key = self.string()?;
                        let key_span = Span {
                            start: key_start,
                            end: self.pos,
                        };
                        self.path.push(PathPart::Member(key.clone()));
                        if let Some(first) = keys.insert(key.clone(), key_span) {
                            let mut error = self.error(
                                "SYN_DUPLICATE_MEMBER",
                                "PARSE",
                                "duplicate decoded key",
                            );
                            error.location.span = key_span;
                            let mut related = *error.location.clone();
                            related.span = first;
                            error.related.push(related);
                            return Err(error);
                        }
                        self.take(b':')?;
                        let value = self.value(depth + 1)?;
                        self.path.pop();
                        members.push(Member {
                            key,
                            key_span,
                            span: Span {
                                start: key_start,
                                end: self.pos,
                            },
                            value,
                        });
                        self.ws();
                        if self.peek() != Some(b',') {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                self.take(b'}')?;
                Value::Object(members)
            }
            Some(b'[') => {
                self.pos += 1;
                self.ws();
                let mut values = Vec::new();
                if self.peek() != Some(b']') {
                    loop {
                        self.path.push(PathPart::Index(values.len()));
                        values.push(self.value(depth + 1)?);
                        self.path.pop();
                        self.ws();
                        if self.peek() != Some(b',') {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                self.take(b']')?;
                Value::Array(values)
            }
            Some(b'"') => Value::String(self.string()?),
            Some(b'-' | b'0'..=b'9') => {
                if self.peek() == Some(b'-') {
                    self.pos += 1;
                }
                match self.peek() {
                    Some(b'0') => self.pos += 1,
                    Some(b'1'..=b'9') => self.digits(),
                    _ => return Err(self.error("SYN_INVALID_TOKEN", "LEX", "invalid number")),
                }
                if self.peek() == Some(b'.') {
                    self.pos += 1;
                    self.required_digits()?;
                }
                if matches!(self.peek(), Some(b'e' | b'E')) {
                    self.pos += 1;
                    if matches!(self.peek(), Some(b'+' | b'-')) {
                        self.pos += 1;
                    }
                    self.required_digits()?;
                }
                if !self.delimited() {
                    return Err(self.error("SYN_INVALID_TOKEN", "LEX", "invalid number suffix"));
                }
                Value::Number(self.source[start..self.pos].into())
            }
            Some(b't' | b'f' | b'n') => {
                let (word, value) = match self.peek() {
                    Some(b't') => ("true", Value::Bool(true)),
                    Some(b'f') => ("false", Value::Bool(false)),
                    _ => ("null", Value::Null),
                };
                if !self.source[self.pos..].starts_with(word) {
                    return Err(self.error("SYN_INVALID_TOKEN", "LEX", "invalid literal"));
                }
                self.pos += word.len();
                if !self.delimited() {
                    return Err(self.error("SYN_INVALID_TOKEN", "LEX", "invalid literal suffix"));
                }
                value
            }
            None | Some(b'}' | b']' | b',') => {
                return Err(self.error("SYN_EXPECTED_TOKEN", "PARSE", "expected value"));
            }
            _ => return Err(self.error("SYN_INVALID_TOKEN", "LEX", "unrecognized token")),
        };
        Ok(Node {
            span: Span {
                start,
                end: self.pos,
            },
            value,
        })
    }
    fn delimited(&self) -> bool {
        matches!(
            self.peek(),
            None | Some(b' ' | b'\t' | b'\r' | b'\n' | b',' | b']' | b'}')
        )
    }
    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
    }
    fn required_digits(&mut self) -> Result<(), Diagnostic> {
        let start = self.pos;
        self.digits();
        if start == self.pos {
            return Err(self.error("SYN_INVALID_TOKEN", "LEX", "expected number digits"));
        }
        Ok(())
    }
}
