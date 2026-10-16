use crate::{Diagnostic, Document, Location, Node, PathPart, Span, Value, parse};
use gateway_domain::semantic_epistemic::InterpretationConfidence;
use gateway_domain::*;
use serde::{Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;

macro_rules! operands {
    ($( $variant:ident: $ty:ty => ($source:literal, $wire:literal, $default:literal) ),+ $(,)?) => {
        /// Typed source operands. Variants preserve the closed CGSL construct registry.
        #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
        #[serde(untagged)]
        pub enum Operand { $( $variant($ty), )+ }
        pub(crate) const MAPPING: &[(&str, &str, &str)] = &[$(($source, $wire, $default)),+];
        fn operand(ctx: &Context<'_>, field: &str, node: &Node, path: Vec<PathPart>) -> Result<Operand, Diagnostic> {
            let result = match field {
                $($source => ctx.decode::<$ty>(node, path.clone()).map(Operand::$variant),)+
                _ => Err(ctx.error("SEM_INVALID_ATTACHMENT", node, path.clone(), "field must name a mapped construct")),
            }?;
            ctx.validate_operand(&result, node, path)?;
            Ok(result)
        }
    }
}

operands! {
    Task: SemanticTaskType => ("task", "task_type", ""),
    Goal: SemanticGoal => ("goal", "goal", ""),
    Target: SemanticTarget => ("target", "target", ""),
    Input: Vec<SemanticTaskInput> => ("input", "inputs", "[]"),
    Context: Vec<ResolvedReference<DeclarativeContextId>> => ("context", "context_refs", "[]"),
    State: Option<ResolvedReference<ObservedStateId>> => ("state", "current_state", "null"),
    DesiredState: Option<DesiredState> => ("desired_state", "desired_state", "null"),
    Observation: Vec<ResolvedReference<ObservationId>> => ("observation", "observations", "[]"),
    History: Vec<ResolvedReference<ReferenceId>> => ("history", "history", "[]"),
    Constraint: Vec<Constraint> => ("constraint", "constraints", "[]"),
    Requires: Vec<ResolvedReference<CapabilityId>> => ("requires", "capability_requirements", "[]"),
    Evidence: Vec<ResolvedReference<EvidenceId>> => ("evidence", "evidence_refs", "[]"),
    Assumption: Vec<SemanticAssumption> => ("assumption", "assumptions", "[]"),
    Policy: Vec<ResolvedReference<PolicyId>> => ("policy", "policy_refs", "[]"),
    Output: SemanticOutputContract => ("output", "output_contract", ""),
    Verification: SemanticVerificationContract => ("verification", "verification_contract", ""),
    Process: Option<gateway_domain::learning::ProcessReference> => ("process", "process", "null"),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceAmbiguity {
    pub field: String,
    pub requirement: SemanticFieldRequirement,
    pub candidates: ResolutionCandidates<Operand>,
}

/// Inspection candidate, not a resolved or executable task. The AST retains all
/// original source locations and sidecars, including unresolved mandatory meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledSource {
    pub document: Document,
    pub id: TaskId,
    pub operands: BTreeMap<String, Operand>,
    pub knowledge_gaps: Vec<KnowledgeGap>,
    pub ambiguities: Vec<SourceAmbiguity>,
    pub confidence: BTreeMap<String, InterpretationConfidence<Operand>>,
    shape: Option<SemanticTaskIR>,
}

impl CompiledSource {
    /// Validated task shape only. The caller must still validate captured pins
    /// and completeness before handoff. Sidecars require interpretation first.
    pub fn task_shape(&self) -> Option<&SemanticTaskIR> {
        self.shape.as_ref()
    }
}

/// Parse, check version, admit typed operands, and prepare domain task inputs.
/// Syntax/version failures stop processing; semantic errors never return a candidate.
pub fn compile(source_id: &str, source: &[u8]) -> Result<CompiledSource, Vec<Diagnostic>> {
    let document = parse(source_id, source).map_err(|e| vec![e])?;
    let ctx = Context {
        document: &document,
    };
    ctx.version().map_err(|e| vec![e])?;
    let result = ctx.compile();
    result.map(
        |(id, operands, knowledge_gaps, ambiguities, confidence, shape)| CompiledSource {
            document,
            id,
            operands,
            knowledge_gaps,
            ambiguities,
            confidence,
            shape,
        },
    )
}

type CompiledParts = (
    TaskId,
    BTreeMap<String, Operand>,
    Vec<KnowledgeGap>,
    Vec<SourceAmbiguity>,
    BTreeMap<String, InterpretationConfidence<Operand>>,
    Option<SemanticTaskIR>,
);
struct Context<'a> {
    document: &'a Document,
}
fn field_path(field: &str) -> Vec<PathPart> {
    vec![PathPart::Member(field.into())]
}
fn child(mut path: Vec<PathPart>, field: &str) -> Vec<PathPart> {
    path.push(PathPart::Member(field.into()));
    path
}

impl Context<'_> {
    fn validate_operand(
        &self,
        operand: &Operand,
        node: &Node,
        path: Vec<PathPart>,
    ) -> Result<(), Diagnostic> {
        self.pins(node, path.clone())?;
        let validation = match operand {
            Operand::Goal(goal) => NonEmptyText::new(goal.description.clone()).map(|_| ()),
            Operand::Input(inputs) => inputs.iter().try_for_each(|input| match &input.value {
                SemanticInputValue::Literal(value) => value.validate(),
                _ => Ok(()),
            }),
            Operand::Assumption(values) => {
                values.iter().try_for_each(|value| value.premise.validate())
            }
            _ => Ok(()),
        };
        validation.map_err(|e| self.error("SEM_INVALID_OPERAND", node, path.clone(), e))?;
        if let Value::Array(values) = &node.value {
            let mut identities = BTreeMap::new();
            for (index, value) in values.iter().enumerate() {
                if let Some(identity) = value.member("id").or_else(|| value.member("role")) {
                    if let Some(first) = identities.insert(identity.json(), index) {
                        let mut p = path.clone();
                        p.push(PathPart::Index(index));
                        let mut error = self.error(
                            "SEM_DUPLICATE_IDENTITY",
                            value,
                            p,
                            "duplicate collection identity",
                        );
                        let mut p = path.clone();
                        p.push(PathPart::Index(first));
                        error
                            .related
                            .push(self.document.location(&values[first], p));
                        return Err(error);
                    }
                }
            }
        }
        Ok(())
    }
    fn pins(&self, node: &Node, path: Vec<PathPart>) -> Result<(), Diagnostic> {
        match &node.value {
            Value::Object(members) => {
                if node.member("contract_version").is_some() {
                    if let Some(digest) = node.member("digest") {
                        let valid = matches!(&digest.value, Value::String(s) if s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
                        if !valid {
                            return Err(self.error(
                                "SEM_INVALID_OPERAND",
                                digest,
                                child(path.clone(), "digest"),
                                "pin requires lowercase SHA-256",
                            ));
                        }
                    }
                }
                for member in members {
                    self.pins(&member.value, child(path.clone(), &member.key))?;
                }
            }
            Value::Array(values) => {
                for (index, value) in values.iter().enumerate() {
                    let mut p = path.clone();
                    p.push(PathPart::Index(index));
                    self.pins(value, p)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn error(
        &self,
        code: &'static str,
        node: &Node,
        path: Vec<PathPart>,
        message: impl ToString,
    ) -> Diagnostic {
        Diagnostic::new(
            code,
            "SEMANTIC",
            self.document.location(node, path),
            message,
        )
    }
    fn missing(&self, node: &Node, path: Vec<PathPart>) -> Diagnostic {
        let mut error = self.error(
            "SEM_MISSING_REQUIRED_FIELD",
            node,
            path,
            "missing required field",
        );
        let end = node.span.end - 1;
        error.location.span = Span { start: end, end };
        error
    }
    fn decode<T: DeserializeOwned>(
        &self,
        node: &Node,
        path: Vec<PathPart>,
    ) -> Result<T, Diagnostic> {
        serde_json::from_str(&node.json())
            .map_err(|e| self.error("SEM_INVALID_OPERAND", node, path, e))
    }
    fn version(&self) -> Result<(), Diagnostic> {
        let root = &self.document.root;
        let node = root.member("cgsl").unwrap_or(root);
        let valid = match &node.value {
            Value::String(v) => {
                let parts: Vec<_> = v.split('.').collect();
                parts.len() == 2
                    && parts.iter().all(|p| {
                        !p.is_empty()
                            && p.bytes().all(|b| b.is_ascii_digit())
                            && (p.len() == 1 || !p.starts_with('0'))
                    })
            }
            _ => false,
        };
        let code = if !valid {
            "VER_INVALID_VERSION"
        } else if node.value != Value::String("1.0".into()) {
            "VER_UNSUPPORTED_VERSION"
        } else {
            return Ok(());
        };
        let mut error = if root.member("cgsl").is_none() {
            self.missing(root, field_path("cgsl"))
        } else {
            self.error(code, node, field_path("cgsl"), "CGSL 1.0 required")
        };
        error.code = code;
        error.phase = "VERSION";
        Err(error)
    }
    fn required<'a>(
        &self,
        node: &'a Node,
        field: &str,
        path: Vec<PathPart>,
    ) -> Result<&'a Node, Diagnostic> {
        node.member(field)
            .ok_or_else(|| self.missing(node, child(path, field)))
    }
    fn closed(&self, node: &Node, fields: &[&str], path: Vec<PathPart>) -> Result<(), Diagnostic> {
        let Value::Object(members) = &node.value else {
            return Err(self.error("SEM_INVALID_OPERAND", node, path, "expected object"));
        };
        for member in members {
            if !fields.contains(&member.key.as_str()) {
                return Err(self.error(
                    "SEM_INVALID_OPERAND",
                    &member.value,
                    child(path, &member.key),
                    "unknown nested member",
                ));
            }
        }
        for field in fields {
            self.required(node, field, path.clone())
                .map_err(|mut error| {
                    error.code = "SEM_INVALID_OPERAND";
                    error
                })?;
        }
        Ok(())
    }
    fn array<'a>(&self, node: &'a Node, path: Vec<PathPart>) -> Result<&'a [Node], Diagnostic> {
        match &node.value {
            Value::Array(values) => Ok(values),
            _ => Err(self.error("SEM_INVALID_OPERAND", node, path, "expected array")),
        }
    }
    fn get<T: DeserializeOwned>(
        &self,
        node: &Node,
        field: &str,
        path: Vec<PathPart>,
    ) -> Result<T, Diagnostic> {
        self.decode(
            self.required(node, field, path.clone())?,
            child(path, field),
        )
    }
    fn gap(&self, node: &Node, path: Vec<PathPart>) -> Result<KnowledgeGap, Diagnostic> {
        self.closed(
            node,
            &[
                "subject",
                "missing_field",
                "reason",
                "provenance",
                "required_resolution_class",
                "requirement",
            ],
            path.clone(),
        )?;
        self.pins(node, path.clone())?;
        let subject = self.required(node, "subject", path.clone())?;
        let sp = child(path.clone(), "subject");
        self.closed(subject, &["kind", "value"], sp.clone())?;
        let kind: String = self.get(subject, "kind", sp.clone())?;
        let subject = match kind.as_str() {
            "TARGET" => KnowledgeGapSubject::Target(self.get(subject, "value", sp)?),
            "PATH" => KnowledgeGapSubject::Path(self.subject_path(subject, "value", sp)?),
            "REQUEST" => KnowledgeGapSubject::Request(self.get(subject, "value", sp)?),
            _ => return Err(self.error("SEM_INVALID_OPERAND", subject, sp, "unknown gap subject")),
        };
        KnowledgeGap::new(
            subject,
            self.subject_path(node, "missing_field", path.clone())?,
            self.get(node, "reason", path.clone())?,
            self.get(node, "provenance", path.clone())?,
            self.get(node, "required_resolution_class", path.clone())?,
            self.get(node, "requirement", path.clone())?,
        )
        .map_err(|e| self.error("SEM_INVALID_ATTACHMENT", node, path, e))
    }
    fn subject_path(
        &self,
        node: &Node,
        field: &str,
        path: Vec<PathPart>,
    ) -> Result<SubjectPath, Diagnostic> {
        let text: String = self.get(node, field, path.clone())?;
        text.parse().map_err(|e: ValidationError| {
            self.error("SEM_INVALID_OPERAND", node, child(path, field), e)
        })
    }
    fn ambiguity(&self, node: &Node, path: Vec<PathPart>) -> Result<SourceAmbiguity, Diagnostic> {
        self.closed(node, &["field", "requirement", "candidates"], path.clone())?;
        let field: String = self.get(node, "field", path.clone())?;
        let candidates_node = self.required(node, "candidates", path.clone())?;
        let cp = child(path.clone(), "candidates");
        let mut candidates = vec![];
        for (index, candidate) in self.array(candidates_node, cp.clone())?.iter().enumerate() {
            let mut p = cp.clone();
            p.push(PathPart::Index(index));
            self.closed(candidate, &["value", "provenance"], p.clone())?;
            let value = operand(
                self,
                &field,
                self.required(candidate, "value", p.clone())?,
                child(p.clone(), "value"),
            )?;
            candidates.push(
                ResolutionCandidate::new(value, self.get(candidate, "provenance", p.clone())?)
                    .map_err(|e| self.error("SEM_INVALID_ATTACHMENT", candidate, p, e))?,
            );
        }
        let candidates = ResolutionCandidates::new(candidates)
            .map_err(|e| self.error("SEM_INVALID_ATTACHMENT", candidates_node, cp, e))?;
        Ok(SourceAmbiguity {
            field,
            requirement: self.get(node, "requirement", path)?,
            candidates,
        })
    }
    fn compile(&self) -> Result<CompiledParts, Vec<Diagnostic>> {
        let root = &self.document.root;
        let Value::Object(members) = &root.value else {
            unreachable!("parser requires object")
        };
        let mut errors = vec![];
        let mut operands = BTreeMap::new();
        let mut gaps: Vec<KnowledgeGap> = vec![];
        let mut ambiguities: Vec<SourceAmbiguity> = vec![];
        let mut confidence = BTreeMap::new();
        let id = self
            .get::<TaskId>(root, "id", vec![])
            .map_err(|e| errors.push(e))
            .ok();
        for member in members {
            let field = member.key.as_str();
            let path = field_path(field);
            let node = &member.value;
            if MAPPING.iter().any(|m| m.0 == field) {
                match operand(self, field, node, path) {
                    Ok(value) => {
                        operands.insert(field.into(), value);
                    }
                    Err(e) => errors.push(e),
                }
            } else if matches!(field, "knowledge_gap" | "ambiguity" | "confidence") {
                let values = match self.array(node, path.clone()) {
                    Ok(v) => v,
                    Err(e) => {
                        errors.push(e);
                        continue;
                    }
                };
                for (index, value) in values.iter().enumerate() {
                    let mut p = path.clone();
                    p.push(PathPart::Index(index));
                    let result = match field {
                        "knowledge_gap" => self.gap(value, p.clone()).and_then(|gap| {
                            if gaps.iter().any(|g| {
                                g.subject() == gap.subject()
                                    && g.missing_field() == gap.missing_field()
                            }) {
                                return Err(self.error(
                                    "SEM_DUPLICATE_IDENTITY",
                                    value,
                                    p.clone(),
                                    "duplicate gap identity",
                                ));
                            }
                            gaps.push(gap);
                            Ok(())
                        }),
                        "ambiguity" => self.ambiguity(value, p.clone()).and_then(|ambiguity| {
                            if ambiguities.iter().any(|a| a.field == ambiguity.field) {
                                return Err(self.error(
                                    "SEM_DUPLICATE_IDENTITY",
                                    value,
                                    p.clone(),
                                    "duplicate ambiguity field",
                                ));
                            }
                            if root
                                .member(&ambiguity.field)
                                .is_some_and(|n| n.value != Value::Null)
                            {
                                return Err(self.error(
                                    "SEM_CONFLICTING_CONTEXT",
                                    value,
                                    p.clone(),
                                    "concrete declaration conflicts with candidates",
                                ));
                            }
                            ambiguities.push(ambiguity);
                            Ok(())
                        }),
                        _ => self
                            .confidence(value, p.clone())
                            .and_then(|(field, attachment)| {
                                if confidence.insert(field, attachment).is_some() {
                                    return Err(self.error(
                                        "SEM_DUPLICATE_IDENTITY",
                                        value,
                                        p.clone(),
                                        "duplicate confidence field",
                                    ));
                                }
                                Ok(())
                            }),
                    };
                    if let Err(e) = result {
                        errors.push(e);
                    }
                }
            } else if !matches!(field, "cgsl" | "id") {
                errors.push(self.error(
                    "SEM_UNSUPPORTED_CONSTRUCT",
                    node,
                    path,
                    "unknown root construct",
                ));
            }
        }
        let mut wire = vec!["\"schema_version\":\"1.0\"".into()];
        if let Some(id) = &id {
            wire.push(format!(
                "\"id\":{}",
                serde_json::to_string(id).expect("ID serialization")
            ));
        }
        let mut complete = true;
        for &(source, target, default) in MAPPING {
            if let Some(node) = root.member(source) {
                wire.push(format!("\"{target}\":{}", node.json()));
            } else if !default.is_empty() {
                wire.push(format!("\"{target}\":{default}"));
            } else {
                complete = false;
                if !ambiguities.iter().any(|a| a.field == source)
                    && !gaps.iter().any(|g| g.missing_field().to_string() == source)
                {
                    errors.push(self.missing(root, field_path(source)));
                }
            }
        }
        let mut shape = None;
        if errors.is_empty() && complete {
            let data: SemanticTaskData = serde_json::from_str(&format!("{{{}}}", wire.join(",")))
                .expect("individually decoded operands");
            match SemanticTaskIR::new(data) {
                Ok(task) => {
                    if gaps.is_empty() && ambiguities.is_empty() && confidence.is_empty() {
                        shape = Some(task);
                    }
                }
                Err(e) => {
                    let code = if matches!(
                        e,
                        ValidationError::InvalidStateCombination {
                            reason: "duplicate semantic collection identity"
                        }
                    ) {
                        "SEM_DUPLICATE_IDENTITY"
                    } else {
                        "SEM_INVALID_OPERAND"
                    };
                    errors.push(self.error(code, root, vec![], e));
                }
            }
        }
        errors.sort_by(|a, b| {
            (
                a.location.span.start,
                a.location.span.end,
                a.code,
                &a.location.path,
            )
                .cmp(&(
                    b.location.span.start,
                    b.location.span.end,
                    b.code,
                    &b.location.path,
                ))
        });
        errors.dedup_by(|later, earlier| {
            if later.code == earlier.code && later.location == earlier.location {
                earlier.related.append(&mut later.related);
                earlier.related.sort();
                earlier.related.dedup();
                true
            } else {
                false
            }
        });
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok((
            id.expect("validated id"),
            operands,
            gaps,
            ambiguities,
            confidence,
            shape,
        ))
    }
    fn confidence(
        &self,
        node: &Node,
        path: Vec<PathPart>,
    ) -> Result<(String, InterpretationConfidence<Operand>), Diagnostic> {
        self.closed(node, &["field", "value", "basis"], path.clone())?;
        self.pins(node, path.clone())?;
        let field: String = self.get(node, "field", path.clone())?;
        let subject = self
            .document
            .root
            .member(&field)
            .filter(|n| n.value != Value::Null)
            .ok_or_else(|| {
                self.error(
                    "SEM_INVALID_ATTACHMENT",
                    node,
                    path.clone(),
                    "confidence requires a present operand",
                )
            })?;
        let result = operand(self, &field, subject, field_path(&field))?;
        Ok((
            field,
            InterpretationConfidence {
                result,
                confidence: self.get(node, "value", path.clone())?,
                basis: self.get(node, "basis", path)?,
            },
        ))
    }
}

/// Find an original node location by decoded path (including array indexes).
pub fn source_location(document: &Document, path: &[PathPart]) -> Option<Location> {
    let mut node = &document.root;
    for part in path {
        node = match (part, &node.value) {
            (PathPart::Member(key), _) => node.member(key)?,
            (PathPart::Index(index), Value::Array(values)) => values.get(*index)?,
            _ => return None,
        };
    }
    Some(document.location(node, path.to_vec()))
}
