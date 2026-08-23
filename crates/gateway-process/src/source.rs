//! Rust-only normalized frontend for the Strict Cognitive Gherkin source shape.

use std::{error::Error, fmt};

/// One-based source position retained for compiler diagnostics and traces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SourceLocation {
    line: usize,
    column: usize,
}

impl SourceLocation {
    #[must_use]
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }

    #[must_use]
    pub const fn line(self) -> usize {
        self.line
    }

    #[must_use]
    pub const fn column(self) -> usize {
        self.column
    }
}

/// A parsed source tag, including its original location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTag {
    name: String,
    value: Option<String>,
    location: SourceLocation,
}

impl SourceTag {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }

    #[must_use]
    pub const fn location(&self) -> SourceLocation {
        self.location
    }
}

/// The Gherkin keyword retained on a normalized step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceStepKeyword {
    Given,
    When,
    Then,
    And,
    But,
}

impl SourceStepKeyword {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Given => "Given",
            Self::When => "When",
            Self::Then => "Then",
            Self::And => "And",
            Self::But => "But",
        }
    }
}

/// A normalized Gherkin data-table row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRow {
    cells: Vec<String>,
    location: SourceLocation,
}

impl TableRow {
    #[must_use]
    pub fn cells(&self) -> &[String] {
        &self.cells
    }

    #[must_use]
    pub const fn location(&self) -> SourceLocation {
        self.location
    }
}

/// A normalized step. The frontend stores text; the semantic compiler decides
/// whether the text is one of the finite authoritative forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceStep {
    keyword: SourceStepKeyword,
    text: String,
    location: SourceLocation,
    table: Vec<TableRow>,
}

impl SourceStep {
    #[must_use]
    pub const fn keyword(&self) -> SourceStepKeyword {
        self.keyword
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub const fn location(&self) -> SourceLocation {
        self.location
    }

    #[must_use]
    pub fn table(&self) -> &[TableRow] {
        &self.table
    }
}

/// A normalized scenario and its ordered steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceScenario {
    name: String,
    location: SourceLocation,
    steps: Vec<SourceStep>,
}

impl SourceScenario {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn location(&self) -> SourceLocation {
        self.location
    }

    #[must_use]
    pub fn steps(&self) -> &[SourceStep] {
        &self.steps
    }
}

/// A normalized Rule. Declarations before the first scenario are retained as
/// ordinary steps so the compiler can apply the normative vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRule {
    name: String,
    location: SourceLocation,
    declarations: Vec<SourceStep>,
    scenarios: Vec<SourceScenario>,
}

impl SourceRule {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn location(&self) -> SourceLocation {
        self.location
    }

    #[must_use]
    pub fn declarations(&self) -> &[SourceStep] {
        &self.declarations
    }

    #[must_use]
    pub fn scenarios(&self) -> &[SourceScenario] {
        &self.scenarios
    }
}

/// The parser-owned normalized source document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDocument {
    process_id: String,
    process_version: String,
    language_version: String,
    feature_name: String,
    feature_location: SourceLocation,
    tags: Vec<SourceTag>,
    rules: Vec<SourceRule>,
}

impl SourceDocument {
    /// Parses one Strict Cognitive Gherkin source document without executing it.
    pub fn parse(source: &str) -> Result<Self, FrontendError> {
        Parser::new(source).parse()
    }

    #[must_use]
    pub fn process_id(&self) -> &str {
        &self.process_id
    }

    #[must_use]
    pub fn process_version(&self) -> &str {
        &self.process_version
    }

    #[must_use]
    pub fn language_version(&self) -> &str {
        &self.language_version
    }

    #[must_use]
    pub fn feature_name(&self) -> &str {
        &self.feature_name
    }

    #[must_use]
    pub const fn feature_location(&self) -> SourceLocation {
        self.feature_location
    }

    #[must_use]
    pub fn tags(&self) -> &[SourceTag] {
        &self.tags
    }

    #[must_use]
    pub fn rules(&self) -> &[SourceRule] {
        &self.rules
    }
}

/// Stable parser diagnostic with source position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontendError {
    code: &'static str,
    message: String,
    location: SourceLocation,
}

impl FrontendError {
    fn new(code: &'static str, message: impl Into<String>, location: SourceLocation) -> Self {
        Self {
            code,
            message: message.into(),
            location,
        }
    }

    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub const fn location(&self) -> SourceLocation {
        self.location
    }
}

impl fmt::Display for FrontendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at {}:{}: {}",
            self.code, self.location.line, self.location.column, self.message
        )
    }
}

impl Error for FrontendError {}

struct Parser<'a> {
    lines: Vec<&'a str>,
    tags: Vec<SourceTag>,
    feature: Option<(String, SourceLocation)>,
    rules: Vec<SourceRule>,
    current_rule: Option<usize>,
    current_scenario: Option<usize>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            lines: source.lines().collect(),
            tags: Vec::new(),
            feature: None,
            rules: Vec::new(),
            current_rule: None,
            current_scenario: None,
        }
    }

    fn parse(mut self) -> Result<SourceDocument, FrontendError> {
        for (index, line) in self.lines.clone().into_iter().enumerate() {
            self.parse_line(index + 1, line)?;
        }
        let (feature_name, feature_location) = self.feature.ok_or_else(|| {
            FrontendError::new(
                "MISSING_FEATURE",
                "Feature declaration is required",
                SourceLocation::new(1, 1),
            )
        })?;
        if self.rules.is_empty() {
            return Err(FrontendError::new(
                "MISSING_RULE",
                "exactly one Process Rule is required",
                feature_location,
            ));
        }
        if self.rules.len() != 1 || self.rules[0].name != "Process" {
            return Err(FrontendError::new(
                "UNSUPPORTED_RULE",
                "Strict Cognitive Gherkin v1 requires one Rule: Process",
                self.rules[0].location,
            ));
        }
        let process_id = required_tag(&self.tags, "process", feature_location)?;
        let process_version = required_tag(&self.tags, "process-version", feature_location)?;
        let language_version = required_tag(&self.tags, "cg-language", feature_location)?;
        Ok(SourceDocument {
            process_id,
            process_version,
            language_version,
            feature_name,
            feature_location,
            tags: self.tags,
            rules: self.rules,
        })
    }

    fn parse_line(&mut self, line_number: usize, raw: &str) -> Result<(), FrontendError> {
        let location = SourceLocation::new(line_number, raw.len() - raw.trim_start().len() + 1);
        let text = raw.trim();
        if text.is_empty() || text.starts_with('#') {
            return Ok(());
        }
        if text.starts_with('@') {
            self.parse_tag(text, location)?;
            return Ok(());
        }
        if let Some(name) = text.strip_prefix("Feature:") {
            if self.feature.is_some() {
                return Err(FrontendError::new(
                    "DUPLICATE_FEATURE",
                    "only one Feature is supported",
                    location,
                ));
            }
            let name = required_text(name, "feature name", location)?;
            self.feature = Some((name.to_owned(), location));
            return Ok(());
        }
        if let Some(name) = text.strip_prefix("Rule:") {
            if self.feature.is_none() || self.current_rule.is_some() {
                return Err(FrontendError::new(
                    "UNSUPPORTED_RULE",
                    "Rule must follow Feature and may occur only once",
                    location,
                ));
            }
            let name = required_text(name, "rule name", location)?;
            self.rules.push(SourceRule {
                name: name.to_owned(),
                location,
                declarations: Vec::new(),
                scenarios: Vec::new(),
            });
            self.current_rule = Some(self.rules.len() - 1);
            self.current_scenario = None;
            return Ok(());
        }
        if let Some(name) = text.strip_prefix("Scenario:") {
            let rule_index = self.current_rule.ok_or_else(|| {
                FrontendError::new(
                    "SCENARIO_OUTSIDE_RULE",
                    "Scenario must be inside Rule: Process",
                    location,
                )
            })?;
            let name = required_text(name, "scenario name", location)?;
            self.rules[rule_index].scenarios.push(SourceScenario {
                name: name.to_owned(),
                location,
                steps: Vec::new(),
            });
            self.current_scenario = Some(self.rules[rule_index].scenarios.len() - 1);
            return Ok(());
        }
        for (keyword, prefix) in [
            (SourceStepKeyword::Given, "Given "),
            (SourceStepKeyword::When, "When "),
            (SourceStepKeyword::Then, "Then "),
            (SourceStepKeyword::And, "And "),
            (SourceStepKeyword::But, "But "),
        ] {
            if let Some(step_text) = text.strip_prefix(prefix) {
                return self.parse_step(keyword, step_text, location);
            }
        }
        for (keyword, bare) in [
            (SourceStepKeyword::Given, "Given"),
            (SourceStepKeyword::When, "When"),
            (SourceStepKeyword::Then, "Then"),
            (SourceStepKeyword::And, "And"),
            (SourceStepKeyword::But, "But"),
        ] {
            if text == bare {
                return self.parse_step(keyword, "", location);
            }
        }
        if text.starts_with('|') {
            return self.parse_table_row(text, location);
        }
        if text == "Background:" || text == "Examples:" || text.starts_with("Scenario Outline:") {
            return Err(FrontendError::new(
                "UNSUPPORTED_STRUCTURE",
                format!("{text} is not supported in v1"),
                location,
            ));
        }
        if self.current_scenario.is_some() {
            return Err(FrontendError::new(
                "INVALID_STEP",
                "expected Given, When, Then, And or But",
                location,
            ));
        }
        Ok(())
    }

    fn parse_tag(&mut self, text: &str, location: SourceLocation) -> Result<(), FrontendError> {
        let body = text.strip_prefix('@').unwrap_or_default();
        let (name, value) = if let Some(open) = body.find('(') {
            if !body.ends_with(')') || open == 0 || open == body.len() - 1 {
                return Err(FrontendError::new(
                    "INVALID_TAG",
                    "tag arguments must use @name(value)",
                    location,
                ));
            }
            (
                &body[..open],
                Some(body[open + 1..body.len() - 1].to_owned()),
            )
        } else {
            (body, None)
        };
        if name.is_empty()
            || !name.chars().all(|character| {
                character.is_ascii_alphanumeric() || character == '-' || character == '_'
            })
        {
            return Err(FrontendError::new(
                "INVALID_TAG",
                "tag name contains unsupported characters",
                location,
            ));
        }
        self.tags.push(SourceTag {
            name: name.to_owned(),
            value,
            location,
        });
        Ok(())
    }

    fn parse_step(
        &mut self,
        keyword: SourceStepKeyword,
        raw_text: &str,
        location: SourceLocation,
    ) -> Result<(), FrontendError> {
        let rule_index = self.current_rule.ok_or_else(|| {
            FrontendError::new(
                "STEP_OUTSIDE_RULE",
                "steps must be inside Rule: Process",
                location,
            )
        })?;
        let text = required_text(raw_text, "step text", location)?;
        let step = SourceStep {
            keyword,
            text: text.to_owned(),
            location,
            table: Vec::new(),
        };
        if let Some(scenario_index) = self.current_scenario {
            self.rules[rule_index].scenarios[scenario_index]
                .steps
                .push(step);
        } else {
            self.rules[rule_index].declarations.push(step);
        }
        Ok(())
    }

    fn parse_table_row(
        &mut self,
        text: &str,
        location: SourceLocation,
    ) -> Result<(), FrontendError> {
        let rule_index = self.current_rule.ok_or_else(|| {
            FrontendError::new(
                "TABLE_OUTSIDE_RULE",
                "tables must follow a process step",
                location,
            )
        })?;
        let scenario_index = self.current_scenario.ok_or_else(|| {
            FrontendError::new(
                "TABLE_OUTSIDE_STEP",
                "tables must be inside a scenario step",
                location,
            )
        })?;
        let cells = text
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if cells.is_empty() || cells.iter().any(String::is_empty) {
            return Err(FrontendError::new(
                "INVALID_TABLE",
                "table cells cannot be empty",
                location,
            ));
        }
        let scenario = &mut self.rules[rule_index].scenarios[scenario_index];
        let step = scenario.steps.last_mut().ok_or_else(|| {
            FrontendError::new("TABLE_OUTSIDE_STEP", "table must follow a step", location)
        })?;
        step.table.push(TableRow { cells, location });
        Ok(())
    }
}

fn required_text<'a>(
    text: &'a str,
    label: &str,
    location: SourceLocation,
) -> Result<&'a str, FrontendError> {
    let text = text.trim();
    if text.is_empty() {
        Err(FrontendError::new(
            "MISSING_TEXT",
            format!("{label} cannot be empty"),
            location,
        ))
    } else {
        Ok(text)
    }
}

fn required_tag(
    tags: &[SourceTag],
    name: &str,
    location: SourceLocation,
) -> Result<String, FrontendError> {
    let values = tags
        .iter()
        .filter(|tag| tag.name == name)
        .collect::<Vec<_>>();
    if values.len() != 1 {
        return Err(FrontendError::new(
            "MISSING_OR_DUPLICATE_TAG",
            format!("exactly one @{name}(...) tag is required"),
            location,
        ));
    }
    let value = values[0].value.as_deref().ok_or_else(|| {
        FrontendError::new(
            "INVALID_TAG",
            format!("@{name} requires a value"),
            values[0].location,
        )
    })?;
    if value.trim().is_empty() {
        return Err(FrontendError::new(
            "INVALID_TAG",
            format!("@{name} requires a non-empty value"),
            values[0].location,
        ));
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = include_str!("../fixtures/strict-cognitive-gherkin/valid.feature");

    #[test]
    fn parses_source_ast_with_locations_and_order() {
        let document = SourceDocument::parse(VALID).unwrap();
        assert_eq!(document.process_id(), "canonical-issue-lifecycle");
        assert_eq!(document.process_version(), "1");
        assert_eq!(document.language_version(), "1");
        assert_eq!(document.rules().len(), 1);
        assert_eq!(document.rules()[0].name(), "Process");
        assert_eq!(document.rules()[0].declarations().len(), 8);
        assert_eq!(
            document.rules()[0].scenarios()[0].steps()[0].keyword(),
            SourceStepKeyword::Given
        );
        assert!(
            document.rules()[0].scenarios()[0].steps()[0]
                .location()
                .line()
                > 0
        );
    }

    #[test]
    fn preserves_tables_and_rejects_unsupported_structures() {
        let source = "@process(example)\n@process-version(1)\n@cg-language(1)\nFeature: Tables\nRule: Process\nGiven state START is initial\nScenario: table\nGiven process state START\n| key | value |\n| one | two |\n";
        let document = SourceDocument::parse(source).unwrap();
        assert_eq!(
            document.rules()[0].scenarios()[0].steps()[0].table()[0].cells(),
            ["key", "value"]
        );
        let unsupported = source.replace("Scenario: table", "Scenario Outline: table");
        assert_eq!(
            SourceDocument::parse(&unsupported).unwrap_err().code(),
            "UNSUPPORTED_STRUCTURE"
        );
    }

    #[test]
    fn fails_with_stable_diagnostics_for_malformed_source() {
        for (source, code) in [
            (
                "Feature: missing tags\nRule: Process",
                "MISSING_OR_DUPLICATE_TAG",
            ),
            (
                "@process(x)\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Other",
                "UNSUPPORTED_RULE",
            ),
            (
                "@process(x)\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process\nGiven",
                "MISSING_TEXT",
            ),
            (
                "@process(x)\n@process-version(1)\n@cg-language(1)\nFeature: x\nRule: Process\nScenario: x\nGiven hello\n| |",
                "INVALID_TABLE",
            ),
        ] {
            assert_eq!(SourceDocument::parse(source).unwrap_err().code(), code);
        }
    }

    #[test]
    fn invalid_fixtures_are_rejected_by_structure_or_semantics_later() {
        let unknown =
            include_str!("../fixtures/strict-cognitive-gherkin/invalid-unknown-step.feature");
        assert!(SourceDocument::parse(unknown).is_ok());
        let missing =
            include_str!("../fixtures/strict-cognitive-gherkin/invalid-missing-initial.feature");
        assert!(SourceDocument::parse(missing).is_ok());
        let version = include_str!("../fixtures/strict-cognitive-gherkin/invalid-version.feature");
        assert_eq!(
            SourceDocument::parse(version).unwrap().language_version(),
            "99"
        );
    }
}
