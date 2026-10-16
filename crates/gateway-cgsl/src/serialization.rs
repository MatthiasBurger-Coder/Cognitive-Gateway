use crate::{Diagnostic, Document, Node, Value, parse};
use gateway_domain::SemanticTaskIR;

/// Canonical source formatting F(AST). Retains sidecars, array order and exact
/// number tokens even for incomplete or semantically invalid documents.
pub fn format_source(document: &Document) -> String {
    fn render(node: &Node, depth: usize) -> String {
        let indent = "  ".repeat(depth + 1);
        let (open, close, entries) = match &node.value {
            Value::Object(members) if !members.is_empty() => {
                let mut sorted: Vec<_> = members.iter().collect();
                sorted.sort_by(|a, b| a.key.cmp(&b.key));
                (
                    '{',
                    '}',
                    sorted
                        .into_iter()
                        .map(|member| {
                            format!(
                                "{indent}{}: {}",
                                serde_json::to_string(&member.key).unwrap(),
                                render(&member.value, depth + 1)
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            }
            Value::Array(values) if !values.is_empty() => (
                '[',
                ']',
                values
                    .iter()
                    .map(|value| format!("{indent}{}", render(value, depth + 1)))
                    .collect(),
            ),
            _ => return node.json(),
        };
        format!(
            "{open}\n{}\n{}{close}",
            entries.join(",\n"),
            "  ".repeat(depth)
        )
    }
    format!("{}\n", render(&document.root, 0))
}

/// Errors while projecting a domain task into the frontend resource profile.
#[derive(Debug)]
pub enum SourceSerializationError {
    Domain(gateway_domain::SerializationError),
    Frontend(Diagnostic),
}

/// Canonical semantic source S(IR). This explicit projection cannot reconstruct
/// interpretation sidecars; it grants no reference, policy or execution authority.
/// Returns a diagnostic if the IR exceeds the frontend's resource profile.
pub fn serialize(task: &SemanticTaskIR) -> Result<String, SourceSerializationError> {
    let canonical = task
        .to_canonical_json()
        .map_err(SourceSerializationError::Domain)?;
    let document = parse("canonical-task", canonical.as_bytes())
        .map_err(SourceSerializationError::Frontend)?;
    let mut fields = vec![
        "\"cgsl\":\"1.0\"".to_owned(),
        format!("\"id\":{}", document.root.member("id").unwrap().json()),
    ];
    for (source, wire, _) in crate::compiler::MAPPING {
        fields.push(format!(
            "\"{source}\":{}",
            document.root.member(wire).unwrap().json()
        ));
    }
    for field in ["knowledge_gap", "ambiguity", "confidence"] {
        fields.push(format!("\"{field}\":[]"));
    }
    let source = format!("{{{}}}", fields.join(","));
    let formatted = format_source(
        &parse("canonical-source", source.as_bytes())
            .map_err(SourceSerializationError::Frontend)?,
    );
    parse("canonical-source", formatted.as_bytes()).map_err(SourceSerializationError::Frontend)?;
    Ok(formatted)
}
