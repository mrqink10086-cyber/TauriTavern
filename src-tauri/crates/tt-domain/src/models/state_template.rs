//! The compiled HTML template for a panel.
//!
//! Level 2 of the panel's style layers: a panel may describe its own markup
//! instead of accepting the default field rows. The editor compiles the text
//! **once, at save time**, into the tree below, and the renderer only walks that
//! tree — it creates elements, sets attributes and writes text, so no markup is
//! ever parsed while a chat is on screen.
//!
//! What the renderer may create is deliberately open: every tag but a short
//! forbidden list, every attribute, and the whole SVG vocabulary, so a panel can
//! draw its own icons rather than waiting for this file to grow. What is refused
//! is markup that amounts to a second document or a second program. The one
//! thing a *value* may never be is code: a binding is checked to name a field
//! the declaration defines, and it is only ever written as text or as part of an
//! attribute, both at render time and here.
//!
//! A document from anywhere else — hand-edited, imported, written by an older
//! version — still has to pass this, because "the editor would not write that"
//! is not a property the renderer can check.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::state_key::{StateKey, StateKeyPattern};
use super::state_panel::StatePanelError;

/// The elements a template may not use.
///
/// Everything else is allowed, the whole SVG vocabulary included. A nested
/// document (`iframe`), a second stylesheet (`style`, `link`) and a second
/// script (`script`) are refused: a panel already has a stylesheet layer and a
/// place to put code, and two of either is one too many to reason about.
pub const FORBIDDEN_TAGS: &[&str] = &[
    "script", "style", "link", "meta", "base", "iframe", "frame", "frameset", "object", "embed",
    "applet", "template", "slot", "noscript",
];

/// The comparisons an `{{#if}}` may carry. Mirrors `TEMPLATE_OPS`.
///
/// Without one, the branch asks whether the field carries anything at all — the
/// question most templates are asking.
pub const TEMPLATE_OPS: &[&str] = &[
    "eq", "ne", "in", "not_in", "contains", "gt", "gte", "lt", "lte", "exists", "missing",
];

/// Ops whose right-hand side is left out: `{{#if 环境/天气 exists}}`.
const UNARY_OPS: &[&str] = &["exists", "missing"];

/// Upper bound on the nodes one template may carry. A panel is a handful of
/// rows; a template is a shape for them, not a document of its own.
pub const MAX_TEMPLATE_NODES: usize = 1000;

/// Upper bound on how deep a template may nest.
pub const MAX_TEMPLATE_DEPTH: usize = 20;

/// Upper bound on one run of literal text.
pub const MAX_TEMPLATE_TEXT_CHARS: usize = 4000;

/// A panel's markup, as the editor compiled it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompiledTemplate {
    #[serde(default)]
    pub nodes: Vec<TemplateNode>,
}

/// One piece of an attribute's value: literal text, or the marker beside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum TemplateAttributePart {
    Text { text: String },
    Value { key: String },
}

/// One node of a compiled template.
///
/// The vocabulary is literal elements and text plus four bindings: a field's
/// value, a branch, a loop over the fields a pattern matches, and the markup
/// around them. Every choice a template makes was made when it was compiled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum TemplateNode {
    /// An element with its attributes and children.
    Element {
        tag: String,
        #[serde(default)]
        attrs: BTreeMap<String, String>,
        /// Attributes whose value is partly a field's; a literal one stays in `attrs`.
        #[serde(default)]
        bound_attrs: BTreeMap<String, Vec<TemplateAttributePart>>,
        #[serde(default)]
        children: Vec<TemplateNode>,
    },
    /// Literal text from the template.
    Text { text: String },
    /// A field's values, inserted as text.
    Value { key: String },
    /// Which of two branches to render, decided by the field's values.
    If {
        key: String,
        /// How the field is compared; empty means "carries anything".
        #[serde(default)]
        op: String,
        /// The right-hand side, empty for the ops that take none.
        #[serde(default)]
        value: String,
        #[serde(rename = "then", default)]
        then_nodes: Vec<TemplateNode>,
        #[serde(rename = "else", default)]
        else_nodes: Vec<TemplateNode>,
    },
    /// The body once per field the pattern matches.
    Each {
        key: String,
        #[serde(default)]
        body: Vec<TemplateNode>,
    },
}

impl CompiledTemplate {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// Check a compiled template before it is stored.
///
/// Everything here is structural, because a tree can be checked without parsing
/// anything: the shape of the markup, and whether each binding names a field the
/// declaration defines. What the template *means* is the renderer's business,
/// and it only ever creates what this function allowed.
pub fn validate_template(
    template: &CompiledTemplate,
    declared_keys: &[StateKeyPattern],
) -> Vec<StatePanelError> {
    let mut errors = Vec::new();
    let mut count = 0usize;
    check_nodes(&template.nodes, declared_keys, 0, &mut count, &mut errors, false);
    if count > MAX_TEMPLATE_NODES {
        errors.push(invalid(format!(
            "the template has more than {MAX_TEMPLATE_NODES} nodes"
        )));
    }
    errors
}

fn invalid(reason: String) -> StatePanelError {
    StatePanelError::InvalidTemplate { reason }
}

/// `on*` is matched by shape: the set of events belongs to the browser.
fn is_event_attribute(name: &str) -> bool {
    match name.strip_prefix("on") {
        Some(rest) => !rest.is_empty() && rest.chars().all(|c| c.is_ascii_lowercase()),
        None => false,
    }
}

fn check_nodes(
    nodes: &[TemplateNode],
    declared_keys: &[StateKeyPattern],
    depth: usize,
    count: &mut usize,
    errors: &mut Vec<StatePanelError>,
    in_each: bool,
) {
    if depth > MAX_TEMPLATE_DEPTH {
        errors.push(invalid(format!(
            "the template nests deeper than {MAX_TEMPLATE_DEPTH} levels"
        )));
        return;
    }

    for node in nodes {
        *count += 1;
        match node {
            TemplateNode::Element {
                tag,
                bound_attrs,
                children,
                ..
            } => {
                if FORBIDDEN_TAGS.contains(&tag.as_str()) {
                    errors.push(invalid(format!(
                        "`<{tag}>` cannot be used in a panel template"
                    )));
                }
                for (name, parts) in bound_attrs {
                    // A handler is code, so a binding inside one would be building
                    // code out of state; the attribute takes what the template says.
                    if is_event_attribute(name) {
                        errors.push(invalid(format!(
                            "a binding cannot be used inside `{name}`: a handler is code"
                        )));
                        continue;
                    }
                    for part in parts {
                        if let TemplateAttributePart::Value { key } = part {
                            check_binding(key, declared_keys, in_each, errors);
                        }
                    }
                }
                check_nodes(children, declared_keys, depth + 1, count, errors, in_each);
            }
            TemplateNode::Text { text } => {
                if text.chars().count() > MAX_TEMPLATE_TEXT_CHARS {
                    errors.push(invalid(format!(
                        "one run of text is longer than {MAX_TEMPLATE_TEXT_CHARS} characters"
                    )));
                }
            }
            TemplateNode::Value { key } => check_binding(key, declared_keys, in_each, errors),
            TemplateNode::If {
                key,
                op,
                value,
                then_nodes,
                else_nodes,
            } => {
                check_binding(key, declared_keys, in_each, errors);
                check_condition(op, value, errors);
                check_nodes(then_nodes, declared_keys, depth + 1, count, errors, in_each);
                check_nodes(else_nodes, declared_keys, depth + 1, count, errors, in_each);
            }
            TemplateNode::Each { key, body } => {
                // A loop needs a pattern: the body renders once per field it
                // matches, and one field is not a loop.
                if StateKeyPattern::parse(key).is_err() || !key.contains('*') {
                    errors.push(invalid(format!(
                        "`{{#each {key}}}` needs a key pattern such as 角色/*/好感度"
                    )));
                }
                check_nodes(body, declared_keys, depth + 1, count, errors, true);
            }
        }
    }
}

fn check_condition(op: &str, value: &str, errors: &mut Vec<StatePanelError>) {
    if op.is_empty() {
        return;
    }
    if !TEMPLATE_OPS.contains(&op) {
        errors.push(invalid(format!(
            "`{op}` is not a comparison a template can make"
        )));
        return;
    }
    let unary = UNARY_OPS.contains(&op);
    if unary && !value.is_empty() {
        errors.push(invalid(format!("`{op}` takes no value to compare with")));
    }
    if !unary && value.is_empty() {
        errors.push(invalid(format!("`{op}` needs a value to compare with")));
    }
}

/// A binding names one field the declaration defines.
///
/// A pattern is refused rather than resolved: `角色/*/好感度` covers many keys,
/// and "which of them did you mean" is a question a template cannot answer at
/// render time without the key space being searched again.
fn check_binding(
    key: &str,
    declared_keys: &[StateKeyPattern],
    allow_pattern: bool,
    errors: &mut Vec<StatePanelError>,
) {
    // Inside a loop a binding names the loop's pattern: "this row's field".
    if allow_pattern && key.contains('*') {
        if StateKeyPattern::parse(key).is_err() {
            errors.push(invalid(format!("`{key}` is not a key pattern")));
        }
        return;
    }
    if StateKey::parse(key).is_err() {
        errors.push(invalid(format!(
            "`{key}` is not a field key: a binding names one field, not a pattern"
        )));
        return;
    }
    if !declared_keys
        .iter()
        .any(|pattern| pattern.matches_exact(key))
    {
        errors.push(invalid(format!(
            "`{key}` is not a field this declaration defines"
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CompiledTemplate, MAX_TEMPLATE_DEPTH, MAX_TEMPLATE_NODES, TemplateAttributePart,
        TemplateNode, validate_template,
    };
    use crate::models::state_key::StateKeyPattern;
    use crate::models::state_panel::StatePanelError;

    fn declared() -> Vec<StateKeyPattern> {
        vec![
            StateKeyPattern::parse("环境/日期").expect("pattern"),
            StateKeyPattern::parse("角色/*/好感度").expect("pattern"),
        ]
    }

    fn element(tag: &str, children: Vec<TemplateNode>) -> TemplateNode {
        TemplateNode::Element {
            tag: tag.to_string(),
            attrs: Default::default(),
            bound_attrs: Default::default(),
            children,
        }
    }

    #[test]
    fn a_template_that_only_uses_what_the_renderer_can_create_is_accepted() {
        let template = CompiledTemplate {
            nodes: vec![
                element(
                    "div",
                    vec![
                        TemplateNode::Text {
                            text: "今天 ".to_string(),
                        },
                        TemplateNode::Value {
                            key: "环境/日期".to_string(),
                        },
                        TemplateNode::If {
                            key: "角色/爱丽丝/好感度".to_string(),
                            op: "gte".to_string(),
                            value: "60".to_string(),
                            then_nodes: vec![TemplateNode::Text {
                                text: "好".to_string(),
                            }],
                            else_nodes: Vec::new(),
                        },
                    ],
                ),
                TemplateNode::Element {
                    tag: "img".to_string(),
                    attrs: [("src".to_string(), "/backgrounds/a.png".to_string())]
                        .into_iter()
                        .collect(),
                    bound_attrs: Default::default(),
                    children: Vec::new(),
                },
            ],
        };

        assert_eq!(validate_template(&template, &declared()), Vec::new());
    }

    #[test]
    fn a_second_document_or_program_is_refused_by_name() {
        for tag in ["script", "style", "iframe", "object", "template"] {
            let errors = validate_template(&CompiledTemplate { nodes: vec![element(tag, Vec::new())] }, &declared());

            assert_eq!(errors.len(), 1, "one refusal for `{tag}`");
            match &errors[0] {
                StatePanelError::InvalidTemplate { reason } => {
                    assert!(reason.contains(tag), "the refusal must name what it refused, got `{reason}`");
                }
                other => panic!("expected an invalid template, got {other:?}"),
            }
        }
    }

    #[test]
    fn the_whole_svg_vocabulary_and_the_open_attributes_are_allowed() {
        let icon = CompiledTemplate {
            nodes: vec![
                TemplateNode::Element {
                    tag: "svg".to_string(),
                    attrs: [
                        ("viewBox".to_string(), "0 0 24 24".to_string()),
                        ("style".to_string(), "width: 1em".to_string()),
                        ("id".to_string(), "mood-icon".to_string()),
                        ("onclick".to_string(), "this.classList.toggle('on')".to_string()),
                    ]
                    .into_iter()
                    .collect(),
                    bound_attrs: Default::default(),
                    children: vec![TemplateNode::Element {
                        tag: "path".to_string(),
                        attrs: [("d".to_string(), "M4 4h16v16H4z".to_string())]
                            .into_iter()
                            .collect(),
                        bound_attrs: Default::default(),
                        children: Vec::new(),
                    }],
                },
                TemplateNode::Element {
                    tag: "div".to_string(),
                    attrs: Default::default(),
                    bound_attrs: [(
                        "style".to_string(),
                        vec![
                            TemplateAttributePart::Text {
                                text: "width: ".to_string(),
                            },
                            TemplateAttributePart::Value {
                                key: "角色/爱丽丝/好感度".to_string(),
                            },
                        ],
                    )]
                    .into_iter()
                    .collect(),
                    children: Vec::new(),
                },
            ],
        };

        assert_eq!(validate_template(&icon, &declared()), Vec::new());
    }

    #[test]
    fn a_handler_takes_no_binding_because_it_is_code() {
        let template = CompiledTemplate {
            nodes: vec![TemplateNode::Element {
                tag: "div".to_string(),
                attrs: Default::default(),
                bound_attrs: [(
                    "onclick".to_string(),
                    vec![TemplateAttributePart::Value {
                        key: "环境/日期".to_string(),
                    }],
                )]
                .into_iter()
                .collect(),
                children: Vec::new(),
            }],
        };

        assert!(matches!(
            validate_template(&template, &declared()).as_slice(),
            [StatePanelError::InvalidTemplate { .. }]
        ));
    }

    #[test]
    fn a_comparison_has_to_be_one_the_renderer_knows() {
        let with_op = |op: &str, value: &str| CompiledTemplate {
            nodes: vec![TemplateNode::If {
                key: "环境/日期".to_string(),
                op: op.to_string(),
                value: value.to_string(),
                then_nodes: Vec::new(),
                else_nodes: Vec::new(),
            }],
        };

        assert_eq!(validate_template(&with_op("gt", "3"), &declared()), Vec::new());
        assert_eq!(validate_template(&with_op("exists", ""), &declared()), Vec::new());
        // Unknown op, an op that takes no value, and one that needs it.
        assert!(!validate_template(&with_op("like", "3"), &declared()).is_empty());
        assert!(!validate_template(&with_op("exists", "3"), &declared()).is_empty());
        assert!(!validate_template(&with_op("gt", ""), &declared()).is_empty());
    }

    #[test]
    fn a_loop_needs_a_pattern() {
        let each = |key: &str| CompiledTemplate {
            nodes: vec![TemplateNode::Each {
                key: key.to_string(),
                body: Vec::new(),
            }],
        };

        assert_eq!(validate_template(&each("角色/*/好感度"), &declared()), Vec::new());
        // One field is not a loop, and a key is not a pattern.
        assert!(!validate_template(&each("角色/爱丽丝/好感度"), &declared()).is_empty());
    }

    #[test]
    fn a_binding_must_name_a_field_the_declaration_defines() {
        for key in [
            // Not declared at all.
            "环境/天气",
            // Declared, but as a pattern: a binding cannot mean many keys.
            "角色/*/好感度",
            // Not a key shape at all.
            "环境/",
        ] {
            let template = CompiledTemplate {
                nodes: vec![TemplateNode::Value {
                    key: key.to_string(),
                }],
            };

            let errors = validate_template(&template, &declared());

            assert_eq!(errors.len(), 1, "`{key}` must be refused");
            assert!(
                matches!(errors[0], StatePanelError::InvalidTemplate { .. }),
                "`{key}` must be refused as a template problem"
            );
        }

        // A literal key a declared pattern covers is exactly what a binding is.
        let covered = CompiledTemplate {
            nodes: vec![TemplateNode::Value {
                key: "角色/爱丽丝/好感度".to_string(),
            }],
        };
        assert_eq!(validate_template(&covered, &declared()), Vec::new());
    }

    #[test]
    fn a_template_cannot_grow_without_bound() {
        let many = CompiledTemplate {
            nodes: vec![TemplateNode::Text { text: "x".to_string() }; MAX_TEMPLATE_NODES + 1],
        };
        assert!(matches!(
            validate_template(&many, &declared()).as_slice(),
            [StatePanelError::InvalidTemplate { .. }]
        ));

        // One level past the limit is refused; at the limit it is not.
        let nest = |levels: usize| {
            let mut node = TemplateNode::Text {
                text: "deep".to_string(),
            };
            for _ in 0..levels {
                node = element("div", vec![node]);
            }
            CompiledTemplate { nodes: vec![node] }
        };
        assert_eq!(validate_template(&nest(MAX_TEMPLATE_DEPTH), &declared()), Vec::new());
        assert!(matches!(
            validate_template(&nest(MAX_TEMPLATE_DEPTH + 2), &declared()).as_slice(),
            [StatePanelError::InvalidTemplate { .. }]
        ));
    }

}
