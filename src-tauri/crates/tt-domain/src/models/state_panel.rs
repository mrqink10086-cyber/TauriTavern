//! How state looks: panels, rails, and the picture each element shows.
//!
//! The declaration says what state exists. This module says how it is shown —
//! and nothing here is written by the model, so a panel can never invent state.
//!
//! Pictures are a **set** with conditions, not one fixed file: a portrait that
//! changes with a relationship value, a background that changes with the scene.
//! Selection is deterministic — first candidate whose condition holds, in
//! declaration order — which is the same shape the state machine uses to pick
//! between competing rules, so users learn one idea rather than two.
//!
//! The condition vocabulary is the state machine's, on purpose: two condition
//! languages that mean almost the same thing would drift apart.

use std::collections::BTreeMap;

use super::agent::WorkspacePath;
use serde::{Deserialize, Serialize};

use super::script_spec::ScriptSpec;
use super::state::{StateDeclaration, StateDocument, StateField};
use super::state_key::{PatternOverlap, StateKey, StateKeyPattern, check_pattern_overlaps};
use super::state_template::{CompiledTemplate, validate_template};
use super::state_machine::{
    ComparatorRegistry, ConditionContext, ConditionOutcome, ConditionSpec, condition_shape_errors,
    evaluate_condition,
};

/// Which side of the chat a panel is docked to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StatePanelRail {
    #[default]
    Left,
    Right,
}

/// One picture the element may show, and the condition that selects it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateImageCandidate {
    /// A path the host already serves, such as `/backgrounds/portrait.png` or
    /// `/user/images/scene.jpg`. Only a same-origin path is allowed: a panel
    /// must not be able to point the webview at an arbitrary file or site.
    pub source: String,
    /// When this candidate applies. Absent marks the fallback.
    #[serde(default)]
    pub when: Option<ConditionSpec>,
}

/// How a picture fills the box it is given.
///
/// One field does two jobs that want opposite answers. A scene picture is the
/// room the panel stands in, and filling it edge to edge while losing what
/// falls outside is right — a bare strip of panel showing around the picture
/// would read as a mistake. A portrait is somebody, and a person cropped to
/// whatever the panel's aspect happens to keep shows a shoulder where a face
/// belongs. The set says which of the two it is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StateImageFit {
    /// Fill the box, cropping what does not fit.
    #[default]
    Cover,
    /// Fit inside the box, whole.
    Contain,
}

/// The pictures one element may show, in priority order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateImageSet {
    #[serde(default)]
    pub candidates: Vec<StateImageCandidate>,
    /// How the chosen picture fills its box. Covers the panel's own background
    /// and a field's pictures alike, because it is a question about the set
    /// rather than about who asked for it.
    #[serde(default)]
    pub fit: StateImageFit,
    /// An optional script that decides which candidate applies.
    ///
    /// Absent is the default and the common case: the candidates' own conditions
    /// decide, in declaration order. A script is the escape hatch for a choice
    /// that conditions cannot express, and it **replaces** the conditions rather
    /// than adding to them — running both would make the shown picture depend on
    /// which of the two a caller happened to apply. The editor says so where the
    /// script is written, so nobody has to guess which one is in force.
    ///
    /// The domain never runs a script: the application layer runs it and hands
    /// the outcome back as a resolved set (see `apply_condition_scripts`).
    #[serde(default)]
    pub condition_script: Option<ScriptSpec>,
}

/// How a field's value is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StateFieldRender {
    /// The values are text.
    #[default]
    Text,
    /// The values name pictures, so they are shown as pictures.
    Image,
}

/// Display settings for the fields one panel matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatePanelFieldSpec {
    pub pattern: StateKeyPattern,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub render: StateFieldRender,
    /// Pictures chosen by state, for a field whose picture is not its value.
    /// When configured it wins over the value, so a field can be both readable
    /// text and a state-driven picture.
    #[serde(default)]
    pub images: Option<StateImageSet>,
}

/// One panel: the keys it shows, where it is docked, and its own picture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatePanelSpec {
    pub title: String,
    #[serde(default)]
    pub rail: StatePanelRail,
    /// The keys this panel shows. Anchored like every other declared pattern,
    /// so `环境/*` does not swallow a deeper key by accident.
    #[serde(rename = "match")]
    pub match_pattern: StateKeyPattern,
    /// The panel's own background, chosen by state.
    #[serde(default)]
    pub background: Option<StateImageSet>,
    #[serde(default)]
    pub fields: Vec<StatePanelFieldSpec>,
    /// The panel's own markup, compiled by the editor at save time.
    ///
    /// It is named for what it is: a tree of elements and bindings, not a
    /// template anyone evaluates later. Absent means the default — one row per
    /// field inside the panel body — and the header and body stay either way, so
    /// a theme written against them keeps working on a panel with markup too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markup: Option<CompiledTemplate>,
    /// A block of prose this panel shows, kept as a file instead of a field.
    ///
    /// A diary entry or a character's inner voice is text somebody writes, not a
    /// fact the model compares: it has no key, no switches, and no place in the
    /// 512-character value limit — which is exactly what makes the state
    /// document a poor home for it. It lives in the run's persistent content
    /// instead, so it travels with the floor through the same version chain the
    /// state does, and it is written with the tools that already edit files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prose: Option<StateProseSpec>,
}

/// Where a panel's prose comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateProseSpec {
    /// A path inside the run's persistent content, such as `persist/heart.md`.
    ///
    /// Only `persist/` can be named: that is the directory whose files are
    /// published with the floor and inherited by the next one, so it is the only
    /// place a panel could read from the version it is showing.
    pub path: String,
    /// The heading the panel shows above the text.
    #[serde(default)]
    pub title: String,
}

/// The prefix every prose path must start with.
pub const PROSE_PATH_PREFIX: &str = "persist/";

/// The user's panel configuration.
///
/// Empty means "not configured yet", and the panels are derived from the
/// declaration instead: one panel per top-level key segment, on the left rail,
/// showing text. A chat with no configuration still gets a usable panel.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatePanelConfig {
    #[serde(default)]
    pub panels: Vec<StatePanelSpec>,
    /// One logic, several picture sets: the modules their scripts may import.
    ///
    /// A set's own script can then stay one line —
    /// `import { background } from './scene.js'; export default background;` —
    /// while the rules live here, once. The name is flat (`scene.js`) because
    /// that is exactly what `./scene.js` resolves to from an entry module: a
    /// path would make resolution depend on where the entry module sits, and
    /// neither the editor nor a reader could say what an import means.
    #[serde(default)]
    pub scripts: BTreeMap<String, String>,
    /// The theme stylesheet, layered over the built-in default style.
    ///
    /// Stored as the editor compiled it: every selector already carries the
    /// panel root, so the text is applied by assigning it once and nothing has
    /// to be parsed at render time. The domain does not read this document — it
    /// checks the mechanical rules in [`validate_theme_css`] and leaves the
    /// meaning of a stylesheet to the browser.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub css: String,
}

impl StatePanelConfig {
    /// Whether any panel here names that prose file.
    ///
    /// Prose is written by path, and a path is the one thing a panel write could
    /// otherwise use to reach any file in the workspace. This is the whole of the
    /// rule that stops it: what a declaration names is what may be written.
    pub fn declares_prose(&self, path: &str) -> bool {
        let wanted = path.trim();
        !wanted.is_empty()
            && self
                .panels
                .iter()
                .filter_map(|panel| panel.prose.as_ref())
                .any(|prose| prose.path.trim() == wanted)
    }
}

/// Safety bound on the part of a module name before `.js`.
pub const MAX_SCRIPT_MODULE_STEM_CHARS: usize = 48;

/// Security bound on a theme stylesheet, counted in characters.
pub const MAX_THEME_CSS_CHARS: usize = 16_384;

/// Whether a shared module name can be imported as written.
pub fn is_script_module_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".js") else {
        return false;
    };
    !stem.is_empty()
        && stem.chars().count() <= MAX_SCRIPT_MODULE_STEM_CHARS
        && stem
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
}

impl StatePanelConfig {
    pub fn is_empty(&self) -> bool {
        self.panels.is_empty()
    }

    /// Panels that can claim the same key, for save-time validation.
    pub fn overlaps(&self) -> Vec<PatternOverlap> {
        let patterns = self
            .panels
            .iter()
            .map(|panel| panel.match_pattern.clone())
            .collect::<Vec<_>>();
        check_pattern_overlaps(&patterns)
    }
}

/// What one panel shows, ready to render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPanel {
    pub title: String,
    pub rail: StatePanelRail,
    /// The background the state selected, if the panel has candidates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    /// How that background fills the panel. Absent when there is no background.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_fit: Option<StateImageFit>,
    pub fields: Vec<ResolvedPanelField>,
    /// The panel's own markup, when it has one.
    ///
    /// It travels with the resolved data because the renderer needs both to draw
    /// anything: the markup says where things go, the fields say what they are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markup: Option<CompiledTemplate>,
    /// The prose block, with its text read from the floor's own version.
    ///
    /// The domain leaves this empty — reading a file is not its job — and the
    /// caller fills it in from the same published version the fields came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prose: Option<ResolvedProse>,
}

/// A prose block, as the panel shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedProse {
    pub title: String,
    pub text: String,
    /// The file the text was read from.
    ///
    /// It travels with the block so an edit knows where to write: prose is a
    /// file somebody owns, not a key the panel could address, and the reader
    /// that read it is the only one that knows which file that was.
    pub path: String,
}

/// One field of a panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPanelField {
    pub key: String,
    pub label: String,
    pub body: ResolvedFieldBody,
}

/// What a field shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ResolvedFieldBody {
    /// The field's values, as text.
    Text { values: Vec<String> },
    /// Pictures, either the field's own values or the set the state selected.
    Images {
        sources: Vec<String>,
        #[serde(default)]
        fit: StateImageFit,
    },
}

/// Why a panel configuration or a picture choice cannot be honoured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatePanelError {
    /// Two candidates could apply at the same time and nothing says which wins.
    UnreachableCandidate { source: String },
    /// A picture path that is not a same-origin path.
    InvalidImageSource { source: String },
    /// The condition on a candidate cannot be compared at all.
    UnknownOp { source: String, op: String },
    /// A condition tree that combines nothing, or compares and combines at once.
    InvalidCondition { source: String, reason: String },
    /// A picture set declares a script but gives it no source.
    EmptyConditionScript { location: String },
    /// A shared module whose name could never be imported.
    InvalidScriptModule { name: String },
    /// A theme stylesheet that cannot be used, and why.
    InvalidThemeCss { reason: String },
    /// A panel template that cannot be used, and why.
    InvalidTemplate { reason: String },
    /// A prose path that could never be read from a published version.
    InvalidProsePath { path: String, reason: String },
}

impl std::fmt::Display for StatePanelError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnreachableCandidate { source } => write!(
                formatter,
                "the fallback `{source}` is not last, so every candidate after it can never be selected"
            ),
            Self::InvalidImageSource { source } => write!(
                formatter,
                "image `{source}` must be a host path such as /backgrounds/name.png"
            ),
            Self::UnknownOp { source, op } => write!(
                formatter,
                "image `{source}` uses condition op `{op}`, which is not registered"
            ),
            Self::InvalidCondition { source, reason } => write!(
                formatter,
                "the condition on image `{source}` cannot be read: {reason}"
            ),
            Self::EmptyConditionScript { location } => write!(
                formatter,
                "{location} declares a condition script with no source"
            ),
            Self::InvalidScriptModule { name } => write!(
                formatter,
                "`{name}` is not a module name an entry script could import; use a flat name such as scene.js"
            ),
            Self::InvalidThemeCss { reason } => {
                write!(formatter, "the theme stylesheet cannot be used: {reason}")
            }
            Self::InvalidProsePath { path, reason } => {
                write!(
                    formatter,
                    "prose `{path}` cannot be read from the floor's own version: {reason}"
                )
            }
            Self::InvalidTemplate { reason } => {
                write!(formatter, "the panel template cannot be used: {reason}")
            }
        }
    }
}

/// Check the whole configuration before it is stored.
///
/// Every rule here is one a runtime guess would otherwise hide: overlapping
/// panels would make a key's placement depend on evaluation order, a fallback
/// in the middle would make the candidates after it dead, and an unregistered
/// op would make a picture silently never match.
/// Check a prose block's path.
///
/// The path has to name a file the chat's own version could hold: a workspace
/// path, inside `persist/`, without a traversal segment. Nothing checks that the
/// file exists — it is written while the story runs, so a scene is saved long
/// before its diary has any pages.
pub fn validate_prose(prose: &StateProseSpec) -> Vec<StatePanelError> {
    let path = prose.path.trim();
    if path.is_empty() {
        return vec![StatePanelError::InvalidProsePath {
            path: prose.path.clone(),
            reason: "a path is required".to_string(),
        }];
    }
    if let Err(error) = WorkspacePath::parse(path) {
        return vec![StatePanelError::InvalidProsePath {
            path: prose.path.clone(),
            reason: error.to_string(),
        }];
    }
    match path.strip_prefix(PROSE_PATH_PREFIX) {
        Some(rest) if !rest.is_empty() => Vec::new(),
        _ => vec![StatePanelError::InvalidProsePath {
            path: prose.path.clone(),
            reason: format!("it must start with `{PROSE_PATH_PREFIX}`"),
        }],
    }
}

pub fn validate_panel_config(
    config: &StatePanelConfig,
    comparators: &ComparatorRegistry,
    declared_keys: &[StateKeyPattern],
) -> Vec<StatePanelError> {
    let mut errors = Vec::new();

    // A module with no source yet is not refused: adding one and pasting its code
    // is two steps, and a save between them is the editor's business, not a broken
    // scene. An entry script that imports an empty module fails at render, where
    // the error names the module.
    for name in config.scripts.keys() {
        if !is_script_module_name(name) {
            errors.push(StatePanelError::InvalidScriptModule { name: name.clone() });
        }
    }

    errors.extend(validate_theme_css(&config.css));

    for panel in &config.panels {
        let title = &panel.title;
        if let Some(markup) = panel.markup.as_ref() {
            errors.extend(validate_template(markup, declared_keys));
        }
        if let Some(prose) = panel.prose.as_ref() {
            errors.extend(validate_prose(prose));
        }
        if let Some(background) = panel.background.as_ref() {
            errors.extend(validate_image_set(
                background,
                comparators,
                &format!("panel `{title}` background"),
            ));
        }
        for field in &panel.fields {
            if let Some(images) = field.images.as_ref() {
                errors.extend(validate_image_set(
                    images,
                    comparators,
                    &format!("panel `{title}` field `{}`", field.pattern.as_str()),
                ));
            }
        }
    }

    errors
}

/// The mechanical rules a theme stylesheet has to satisfy.
///
/// Scoping is not checked here: prefixing every selector with the panel root is
/// what the editor does when it compiles the stylesheet, because that is where a
/// CSS parser lives, and a domain that cannot parse a document must not pretend
/// to validate its meaning. What is checked is everything that decides whether
/// the text is still "a stylesheet for the panel": the size, `<` (the text goes
/// inside a `<style>` element, and that character is how it stops being one),
/// `@import` (the panel must not fetch anything), and every `url()` source (the
/// same host-path rule a picture has, for the same reason).
fn validate_theme_css(css: &str) -> Vec<StatePanelError> {
    let mut errors = Vec::new();
    let source = css.trim();
    if source.is_empty() {
        return errors;
    }
    if source.chars().count() > MAX_THEME_CSS_CHARS {
        errors.push(StatePanelError::InvalidThemeCss {
            reason: format!("the stylesheet is longer than {MAX_THEME_CSS_CHARS} characters"),
        });
        return errors;
    }
    if source.contains('<') {
        errors.push(StatePanelError::InvalidThemeCss {
            reason: "a stylesheet cannot contain `<`".to_string(),
        });
    }
    if source.to_ascii_lowercase().contains("@import") {
        errors.push(StatePanelError::InvalidThemeCss {
            reason: "`@import` is not allowed: the panel must not fetch anything".to_string(),
        });
    }
    for url in theme_css_urls(source) {
        if !is_host_image_source(&url) {
            errors.push(StatePanelError::InvalidThemeCss {
                reason: format!(
                    "`url({url})` must be a host path such as /backgrounds/name.png"
                ),
            });
        }
    }
    errors
}

/// Every `url(...)` argument in a stylesheet, read as written.
///
/// This is not a CSS parser: it finds the literal `url(`, honours a quoted
/// argument so a source containing `)` is still read whole, and reads a bare one
/// up to the first `)`. Anything it cannot read comes back as written, which then
/// fails the host-path check rather than being waved through.
fn theme_css_urls(css: &str) -> Vec<String> {
    let lower = css.to_ascii_lowercase();
    let mut sources = Vec::new();
    let mut cursor = 0;
    while let Some(found) = lower[cursor..].find("url(") {
        let start = cursor + found + "url(".len();
        let (source, next) = read_url_argument(css, start);
        sources.push(source);
        cursor = next.max(start);
    }
    sources
}

/// One `url(...)` argument plus the index to continue from.
fn read_url_argument(css: &str, start: usize) -> (String, usize) {
    let rest = &css[start..];
    let trimmed = rest.trim_start();
    let skipped = rest.len() - trimmed.len();
    let body = start + skipped;

    match trimmed.chars().next() {
        Some(quote @ ('"' | '\'')) => {
            let after_quote = body + quote.len_utf8();
            match css[after_quote..].find(quote) {
                Some(end) => {
                    let source = css[after_quote..after_quote + end].to_string();
                    (source, after_quote + end + quote.len_utf8())
                }
                None => (css[after_quote..].trim().to_string(), css.len()),
            }
        }
        _ => match css[body..].find(')') {
            Some(end) => (css[body..body + end].trim().to_string(), body + end + 1),
            None => (rest.trim().to_string(), css.len()),
        },
    }
}

fn validate_image_set(
    set: &StateImageSet,
    comparators: &ComparatorRegistry,
    location: &str,
) -> Vec<StatePanelError> {
    let mut errors = Vec::new();

    if let Some(script) = set.condition_script.as_ref()
        && !script.is_configured()
    {
        errors.push(StatePanelError::EmptyConditionScript {
            location: location.to_string(),
        });
    }

    for (index, candidate) in set.candidates.iter().enumerate() {
        if !is_host_image_source(&candidate.source) {
            errors.push(StatePanelError::InvalidImageSource {
                source: candidate.source.clone(),
            });
        }
        match candidate.when.as_ref() {
            Some(condition) => {
                errors.extend(condition_shape_errors(condition, comparators).into_iter().map(
                    |error| match error {
                        // Kept as its own variant: an unregistered op is the one
                        // shape problem a user can fix by picking another op.
                        super::state_machine::ConditionError::UnknownOp { op } => {
                            StatePanelError::UnknownOp {
                                source: candidate.source.clone(),
                                op,
                            }
                        }
                        other => StatePanelError::InvalidCondition {
                            source: candidate.source.clone(),
                            reason: other.to_string(),
                        },
                    },
                ));
            }
            // A fallback only makes sense last: anything after it is unreachable.
            None if index + 1 < set.candidates.len() => {
                errors.push(StatePanelError::UnreachableCandidate {
                    source: candidate.source.clone(),
                });
            }
            None => {}
        }
    }
    errors
}

/// A picture source has to be a path the host serves, never a scheme or a
/// traversal: the panel is inside the app's own origin.
///
/// Quotes, parentheses, whitespace and control characters are refused as well:
/// the path ends up inside a CSS `url(...)` and an `src` attribute, and neither
/// parser should ever be able to read more than the path.
pub(super) fn is_host_image_source(source: &str) -> bool {
    let source = source.trim();
    source.len() > 1
        && source.starts_with('/')
        && !source.starts_with("//")
        && !source.contains("..")
        && !source.contains(':')
        && !source.contains('\\')
        && !source
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
        && !source.contains(['"', '\'', '(', ')'])
}

/// Select the picture an element shows.
///
/// Returns `None` when nothing matches: an element with no applicable picture
/// simply has none, which is not a failure. An unregistered op is a failure —
/// the alternative is showing a picture the configuration did not choose.
pub fn resolve_image<'a>(
    set: &'a StateImageSet,
    fields: &BTreeMap<String, Vec<String>>,
    comparators: &ComparatorRegistry,
) -> Result<Option<&'a str>, StatePanelError> {
    let active = std::collections::BTreeSet::new();
    let context = ConditionContext {
        fields,
        active: &active,
    };

    for candidate in &set.candidates {
        let Some(condition) = candidate.when.as_ref() else {
            return Ok(Some(candidate.source.as_str()));
        };
        match evaluate_condition(condition, &context, comparators) {
            ConditionOutcome::Holds => return Ok(Some(candidate.source.as_str())),
            ConditionOutcome::DoesNotHold => continue,
            ConditionOutcome::UnknownOp { op } => {
                return Err(StatePanelError::UnknownOp {
                    source: candidate.source.clone(),
                    op,
                });
            }
        }
    }

    Ok(None)
}

/// Build what the panel shows.
///
/// Only fields that the document actually carries are shown: an unwritten field
/// is not state yet, and a panel full of empty rows would say otherwise.
pub fn resolve_panels(
    declaration: &StateDeclaration,
    document: &StateDocument,
    config: &StatePanelConfig,
    comparators: &ComparatorRegistry,
) -> Result<Vec<ResolvedPanel>, StatePanelError> {
    let fields = document.condition_fields();
    let panels = if config.is_empty() {
        derive_panels(declaration)
    } else {
        config.panels.clone()
    };

    let mut resolved = Vec::new();
    for panel in &panels {
        let shown = document
            .fields
            .iter()
            .filter(|field| panel.match_pattern.matches_exact(field.key.as_str()))
            .collect::<Vec<_>>();
        if shown.is_empty() && panel.markup.is_none() {
            // A panel with no data does not exist yet; the design's rule is that
            // the panel shows state, not that it shows a schema. A panel that
            // carries its own markup is the exception: that is an interface its
            // author drew, headings and slots included, and an empty one is still
            // the shape they drew rather than a row per missing field.
            continue;
        }

        let (background, background_fit) = match panel.background.as_ref() {
            Some(set) => match resolve_image(set, &fields, comparators)? {
                Some(source) => (Some(source.to_string()), Some(set.fit)),
                None => (None, None),
            },
            None => (None, None),
        };

        let mut panel_fields = Vec::with_capacity(shown.len());
        for field in shown {
            let spec = panel
                .fields
                .iter()
                .find(|spec| spec.pattern.matches_exact(field.key.as_str()));
            panel_fields.push(resolve_field(declaration, field, spec, &fields, comparators)?);
        }

        resolved.push(ResolvedPanel {
            title: panel.title.clone(),
            rail: panel.rail,
            background,
            background_fit,
            fields: panel_fields,
            markup: panel.markup.clone(),
            // Read by the caller, from the version the fields came from.
            prose: None,
        });
    }

    Ok(resolved)
}

/// One field, resolved the way a panel row would show it.
fn resolve_field(
    declaration: &StateDeclaration,
    field: &StateField,
    spec: Option<&StatePanelFieldSpec>,
    fields: &BTreeMap<String, Vec<String>>,
    comparators: &ComparatorRegistry,
) -> Result<ResolvedPanelField, StatePanelError> {
    let label = spec
        .and_then(|spec| spec.label.clone())
        .or_else(|| declaration_label(declaration, &field.key))
        .unwrap_or_else(|| field.key.as_str().to_string());

    let render = spec.map(|spec| spec.render).unwrap_or_default();
    let body = match (render, spec.and_then(|spec| spec.images.as_ref())) {
        // Configured pictures win over the value: the state chooses the
        // picture, and the value keeps its own meaning.
        (_, Some(set)) => match resolve_image(set, fields, comparators)? {
            Some(source) => ResolvedFieldBody::Images {
                sources: vec![source.to_string()],
                fit: set.fit,
            },
            None => ResolvedFieldBody::Text {
                values: field.values.clone(),
            },
        },
        // A field rendered as pictures but with no set: its values are paths
        // somebody wrote, usually a portrait, and nothing has said the edge of
        // the box is where the picture should stop. Shown whole, because
        // cropping art nobody asked to crop is the worse of the two guesses.
        (StateFieldRender::Image, None) => ResolvedFieldBody::Images {
            sources: field.values.clone(),
            fit: StateImageFit::Contain,
        },
        (StateFieldRender::Text, None) => ResolvedFieldBody::Text {
            values: field.values.clone(),
        },
    };

    Ok(ResolvedPanelField {
        key: field.key.as_str().to_string(),
        label,
        body,
    })
}

/// Every field the document carries, resolved once for the templates.
///
/// A panel's rows are what its `match` covers; a template may bind any field the
/// declaration has, so it needs the whole key space rather than one panel's
/// slice of it. A field's picture and label come from whichever panel configures
/// it — the same spec that field's own row would have used.
pub fn resolve_all_fields(
    declaration: &StateDeclaration,
    document: &StateDocument,
    config: &StatePanelConfig,
    comparators: &ComparatorRegistry,
) -> Result<Vec<ResolvedPanelField>, StatePanelError> {
    let fields = document.condition_fields();
    let mut resolved = Vec::with_capacity(document.fields.len());
    for field in &document.fields {
        let spec = config
            .panels
            .iter()
            .flat_map(|panel| panel.fields.iter())
            .find(|spec| spec.pattern.matches_exact(field.key.as_str()));
        resolved.push(resolve_field(declaration, field, spec, &fields, comparators)?);
    }
    Ok(resolved)
}

/// Panels for a declaration that has no panel configuration: one per top-level
/// key segment, in the order the user declared them.
///
/// `match` is anchored, so a panel covering a whole branch has to say so: a
/// declaration of `环境/**` derives the same pattern, while a single-segment
/// declaration of `日期` keeps its exact name. `环境` alone would claim only a
/// key literally called `环境`, which is how a panel ends up silently empty.
fn derive_panels(declaration: &StateDeclaration) -> Vec<StatePanelSpec> {
    let mut panels: Vec<StatePanelSpec> = Vec::new();
    for field in &declaration.fields {
        let declared = field.pattern.as_str();
        let title = top_level_segment(declared);
        if panels.iter().any(|panel| panel.title == title) {
            continue;
        }
        let match_pattern = if declared.contains('/') {
            format!("{title}/**")
        } else {
            declared.to_string()
        };
        let Ok(match_pattern) = StateKeyPattern::parse(&match_pattern) else {
            continue;
        };
        panels.push(StatePanelSpec {
            title: title.clone(),
            rail: StatePanelRail::Left,
            match_pattern,
            background: None,
            fields: Vec::new(),
            markup: None,
            // Derived panels show fields; prose is something a person writes down.
            prose: None,
        });
    }
    panels
}

/// The first `/`-separated segment, which is what a panel groups by. A leading
/// wildcard has no name to group under, so the whole pattern becomes the title.
fn top_level_segment(pattern: &str) -> String {
    let first = pattern.split('/').next().unwrap_or(pattern);
    if first.contains('*') {
        pattern.to_string()
    } else {
        first.to_string()
    }
}

fn declaration_label(declaration: &StateDeclaration, key: &StateKey) -> Option<String> {
    declaration
        .fields
        .iter()
        .find(|field| field.pattern.matches_exact(key.as_str()))
        .map(|field| field.label.clone())
}

#[cfg(test)]
mod tests {
    use crate::models::state_access::StateFieldAccess;
    use std::collections::BTreeMap;

    use super::{
        MAX_THEME_CSS_CHARS, ResolvedFieldBody, ResolvedPanelField, StateFieldRender,
        StateImageCandidate, StateImageFit,
        StateImageSet, StatePanelConfig, StatePanelError, StatePanelFieldSpec, StatePanelRail,
        StatePanelSpec, resolve_image, resolve_panels, validate_panel_config,
    };
    use crate::models::state::{
        DeclaredStateField, StateDeclaration, StateDocument, StateField, StateKey,
    };
    use crate::models::script_spec::ScriptSpec;
    use crate::models::state_key::StateKeyPattern;
    use crate::models::state_machine::{
        ComparatorRegistry, ConditionCompose, ConditionSpec, SOURCE_FIELD,
    };

    fn key(raw: &str) -> StateKey {
        StateKey::parse(raw).expect("test key must be valid")
    }

    #[test]
    fn only_a_named_prose_file_may_be_written() {
        // The panel's write path addresses prose by path, so the declaration is
        // the only thing standing between it and any file in the workspace.
        let config = StatePanelConfig {
            panels: vec![StatePanelSpec {
                title: "内心".to_string(),
                rail: StatePanelRail::Left,
                match_pattern: StateKeyPattern::parse("角色/*").expect("pattern"),
                background: None,
                fields: Vec::new(),
                markup: None,
                prose: Some(super::StateProseSpec {
                    path: "persist/heart.md".to_string(),
                    title: "heart".to_string(),
                }),
            }],
            ..Default::default()
        };

        assert!(config.declares_prose("persist/heart.md"));
        assert!(config.declares_prose("  persist/heart.md  "));
        assert!(!config.declares_prose("persist/other.md"));
        assert!(!config.declares_prose("state/document.json"));
        assert!(!config.declares_prose(""));
    }

    fn declaration(entries: &[(&str, &str)]) -> StateDeclaration {
        StateDeclaration {
            fields: entries
                .iter()
                .map(|(pattern, label)| DeclaredStateField {
                    pattern: StateKeyPattern::parse(pattern).expect("pattern"),
                    label: label.to_string(),
                    access: StateFieldAccess::DECLARED,
                    initial: Vec::new(),
                })
                .collect(),
            panels: Default::default(),
            machine: None,
            predicates: None,
            limits: Default::default(),
        }
    }

    fn document(fields: &[(&str, &[&str])]) -> StateDocument {
        StateDocument {
            fields: fields
                .iter()
                .map(|(raw, values)| StateField {
                    key: key(raw),
                    values: values.iter().map(|value| value.to_string()).collect(),
                })
                .collect(),
        }
    }

    fn number_at_least(field: &str, value: &str) -> ConditionSpec {
        ConditionSpec {
            source: SOURCE_FIELD.to_string(),
            field: Some(field.to_string()),
            op: "gte".to_string(),
            value: Some(value.to_string()),
            values: Vec::new(),
            compose: None,
        }
    }

    fn candidate(source: &str, when: Option<ConditionSpec>) -> StateImageCandidate {
        StateImageCandidate {
            source: source.to_string(),
            when,
        }
    }

    fn comparators() -> ComparatorRegistry {
        ComparatorRegistry::default()
    }

    #[test]
    fn a_panel_that_has_no_state_does_not_exist_yet() {
        let panels = resolve_panels(
            &declaration(&[("环境/日期", "DATE")]),
            &StateDocument::default(),
            &StatePanelConfig::default(),
            &comparators(),
        )
        .expect("an empty document resolves");

        assert!(panels.is_empty());
    }

    #[test]
    fn an_unconfigured_chat_gets_one_panel_per_top_level_segment() {
        let panels = resolve_panels(
            &declaration(&[("环境/日期", "DATE"), ("角色/艾拉/着装", "OUTFIT")]),
            &document(&[("环境/日期", &["2026/09/10"]), ("角色/艾拉/着装", &["外套"])]),
            &StatePanelConfig::default(),
            &comparators(),
        )
        .expect("panels derive from the declaration");

        assert_eq!(
            panels
                .iter()
                .map(|panel| (panel.title.as_str(), panel.rail))
                .collect::<Vec<_>>(),
            [("环境", StatePanelRail::Left), ("角色", StatePanelRail::Left)]
        );
        assert_eq!(
            panels[1].fields,
            vec![ResolvedPanelField {
                key: "角色/艾拉/着装".to_string(),
                label: "OUTFIT".to_string(),
                body: ResolvedFieldBody::Text {
                    values: vec!["外套".to_string()]
                },
            }]
        );
    }

    fn panel_with_background(candidates: Vec<StateImageCandidate>) -> StatePanelConfig {
        StatePanelConfig {
            css: Default::default(),
            scripts: Default::default(),
            panels: vec![StatePanelSpec {
                title: "环境".to_string(),
                rail: StatePanelRail::Right,
                match_pattern: StateKeyPattern::parse("环境/*").expect("pattern"),
                background: Some(StateImageSet {
                    candidates,
                    condition_script: None,
                    ..Default::default()
                }),
                markup: None,
                fields: Vec::new(),
                prose: None,
            }],
        }
    }

    #[test]
    fn a_single_wildcard_claims_one_level_and_not_the_branch_below_it() {
        // The anchored rule applies to panels exactly as it applies to
        // permissions: `角色/*` is one segment, so a panel that means "everything
        // about a character" has to say `角色/**`.
        let config = StatePanelConfig {
            css: Default::default(),
            scripts: Default::default(),
            panels: vec![StatePanelSpec {
                title: "角色".to_string(),
                rail: StatePanelRail::Left,
                match_pattern: StateKeyPattern::parse("角色/*").expect("pattern"),
                background: None,
                markup: None,
                fields: Vec::new(),
                prose: None,
            }],
        };

        let panels = resolve_panels(
            &declaration(&[("角色/艾拉", "NAME")]),
            &document(&[("角色/艾拉", &["艾拉"]), ("角色/艾拉/好感度", &["72"])]),
            &config,
            &comparators(),
        )
        .expect("the panel resolves");

        assert_eq!(
            panels[0]
                .fields
                .iter()
                .map(|field| field.key.as_str())
                .collect::<Vec<_>>(),
            ["角色/艾拉"]
        );
    }

    #[test]
    fn a_state_condition_picks_which_picture_shows() {
        let config = panel_with_background(vec![
            candidate(
                "/backgrounds/night.png",
                Some(number_at_least("关系/好感", "60")),
            ),
            candidate(
                "/backgrounds/day.png",
                Some(number_at_least("关系/好感", "0")),
            ),
        ]);

        let panels = resolve_panels(
            &declaration(&[("环境/日期", "DATE")]),
            &document(&[("环境/日期", &["2026/09/10"]), ("关系/好感", &["72"])]),
            &config,
            &comparators(),
        )
        .expect("the picture resolves");

        assert_eq!(panels[0].background.as_deref(), Some("/backgrounds/night.png"));
        assert_eq!(panels[0].rail, StatePanelRail::Right);
    }

    #[test]
    fn a_later_condition_wins_when_the_earlier_one_does_not_hold() {
        let config = panel_with_background(vec![
            candidate(
                "/backgrounds/close.png",
                Some(number_at_least("关系/好感", "80")),
            ),
            candidate(
                "/backgrounds/neutral.png",
                Some(number_at_least("关系/好感", "0")),
            ),
        ]);

        let panels = resolve_panels(
            &declaration(&[("环境/日期", "DATE")]),
            &document(&[("环境/日期", &["2026/09/10"]), ("关系/好感", &["12"])]),
            &config,
            &comparators(),
        )
        .expect("the picture resolves");

        assert_eq!(
            panels[0].background.as_deref(),
            Some("/backgrounds/neutral.png")
        );
    }

    #[test]
    fn nothing_matching_leaves_the_element_without_a_picture() {
        let set = StateImageSet {
            candidates: vec![candidate(
                "/backgrounds/close.png",
                Some(number_at_least("关系/好感", "80")),
            )],
            condition_script: None,
            ..Default::default()
        };

        let resolved = resolve_image(
            &set,
            &document(&[("关系/好感", &["10"])]).condition_fields(),
            &comparators(),
        )
        .expect("a miss is not a failure");

        assert_eq!(resolved, None);
    }

    #[test]
    fn a_fallback_answers_when_no_condition_holds() {
        let set = StateImageSet {
            candidates: vec![
                candidate(
                    "/backgrounds/close.png",
                    Some(number_at_least("关系/好感", "80")),
                ),
                candidate("/backgrounds/default.png", None),
            ],
            condition_script: None,
            ..Default::default()
        };

        let resolved = resolve_image(
            &set,
            &document(&[("关系/好感", &["10"])]).condition_fields(),
            &comparators(),
        )
        .expect("the fallback answers");

        assert_eq!(resolved, Some("/backgrounds/default.png"));
    }

    #[test]
    fn an_unregistered_op_is_an_error_not_a_miss() {
        let set = StateImageSet {
            candidates: vec![candidate(
                "/backgrounds/a.png",
                Some(ConditionSpec {
                    source: SOURCE_FIELD.to_string(),
                    field: Some("关系/好感".to_string()),
                    op: "closer_than".to_string(),
                    value: Some("30".to_string()),
                    values: Vec::new(),
                    compose: None,
                }),
            )],
            condition_script: None,
            ..Default::default()
        };

        let error = resolve_image(
            &set,
            &document(&[("关系/好感", &["10"])]).condition_fields(),
            &comparators(),
        )
        .expect_err("showing a picture nothing chose is worse than failing");

        assert!(matches!(error, StatePanelError::UnknownOp { .. }));
    }

    #[test]
    fn a_fallback_before_another_candidate_is_refused_at_save_time() {
        let config = panel_with_background(vec![
            candidate("/backgrounds/default.png", None),
            candidate(
                "/backgrounds/close.png",
                Some(number_at_least("关系/好感", "80")),
            ),
        ]);

        let errors = validate_panel_config(&config, &comparators(), &[]);

        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            StatePanelError::UnreachableCandidate { .. }
        ));
    }

    #[test]
    fn a_picture_source_must_be_a_path_the_host_serves() {
        for source in [
            "https://example.test/scene.png",
            "//example.test/scene.png",
            "/backgrounds/../../secrets.png",
            "backgrounds/scene.png",
            // These end up inside `url(...)` and an `src` attribute, so a quote
            // or a parenthesis must never get through.
            "/backgrounds/sc\"ene.png",
            "/backgrounds/scene).png",
            "/backgrounds/sc ene.png",
        ] {
            let config = panel_with_background(vec![candidate(source, None)]);

            let errors = validate_panel_config(&config, &comparators(), &[]);

            assert!(
                matches!(errors.as_slice(), [StatePanelError::InvalidImageSource { .. }]),
                "`{source}` must be refused, got {errors:?}"
            );
        }
    }

    #[test]
    fn a_configured_picture_wins_over_the_value_it_belongs_to() {
        let config = StatePanelConfig {
            css: Default::default(),
            scripts: Default::default(),
            panels: vec![StatePanelSpec {
                title: "角色".to_string(),
                rail: StatePanelRail::Left,
                match_pattern: StateKeyPattern::parse("角色/**").expect("pattern"),
                background: None,
                markup: None,
                fields: vec![StatePanelFieldSpec {
                    pattern: StateKeyPattern::parse("角色/艾拉/好感度").expect("pattern"),
                    label: Some("好感".to_string()),
                    render: StateFieldRender::Text,
                    images: Some(StateImageSet {
                        candidates: vec![candidate("/user/images/close.png", None)],
                        condition_script: None,
                        ..Default::default()
                    }),
                }],
                prose: None,
            }],
        };

        let panels = resolve_panels(
            &declaration(&[("角色/艾拉/好感度", "AFFECTION")]),
            &document(&[("角色/艾拉/好感度", &["72"])]),
            &config,
            &comparators(),
        )
        .expect("the field resolves");

        assert_eq!(
            panels[0].fields,
            vec![ResolvedPanelField {
                key: "角色/艾拉/好感度".to_string(),
                label: "好感".to_string(),
                body: ResolvedFieldBody::Images {
                    sources: vec!["/user/images/close.png".to_string()],
                    fit: StateImageFit::Cover,
                },
            }],
            "the state chooses the picture; the value stays what it was"
        );
    }

    #[test]
    fn a_field_marked_as_an_image_shows_its_values_as_pictures() {
        let config = StatePanelConfig {
            css: Default::default(),
            scripts: Default::default(),
            panels: vec![StatePanelSpec {
                title: "角色".to_string(),
                rail: StatePanelRail::Left,
                match_pattern: StateKeyPattern::parse("角色/**").expect("pattern"),
                background: None,
                markup: None,
                fields: vec![StatePanelFieldSpec {
                    pattern: StateKeyPattern::parse("角色/*/立绘").expect("pattern"),
                    label: None,
                    render: StateFieldRender::Image,
                    images: None,
                }],
                prose: None,
            }],
        };

        let panels = resolve_panels(
            &declaration(&[("角色/艾拉/立绘", "PORTRAIT")]),
            &document(&[("角色/艾拉/立绘", &["/user/images/aira.png"])]),
            &config,
            &comparators(),
        )
        .expect("the field resolves");

        assert_eq!(
            panels[0].fields[0].body,
            // Shown whole: the value is a path somebody wrote, and no set has
            // said the box's edge is where the picture stops.
            ResolvedFieldBody::Images {
                sources: vec!["/user/images/aira.png".to_string()],
                fit: StateImageFit::Contain,
            }
        );
        assert_eq!(
            panels[0].fields[0].label, "PORTRAIT",
            "a field with no display override keeps the declaration's label"
        );
    }

    fn both_of(parts: Vec<ConditionSpec>) -> ConditionSpec {
        ConditionSpec {
            compose: Some(ConditionCompose::All(parts)),
            ..Default::default()
        }
    }

    #[test]
    fn a_combination_selects_a_picture_the_way_the_user_wrote_it() {
        // "夜晚 and 下雨" is configuration, not a program: the picture is chosen
        // by the same declaration-order rule as a single comparison.
        let set = StateImageSet {
            candidates: vec![
                candidate(
                    "/backgrounds/rainy-night.png",
                    Some(both_of(vec![
                        number_at_least("环境/时间", "18"),
                        ConditionSpec {
                            source: SOURCE_FIELD.to_string(),
                            field: Some("环境/天气".to_string()),
                            op: "eq".to_string(),
                            value: Some("下雨".to_string()),
                            values: Vec::new(),
                            compose: None,
                        },
                    ])),
                ),
                candidate("/backgrounds/day.png", None),
            ],
            condition_script: None,
            ..Default::default()
        };

        let rainy = resolve_image(
            &set,
            &document(&[("环境/时间", &["20"]), ("环境/天气", &["下雨"])]).condition_fields(),
            &comparators(),
        )
        .expect("the combination resolves")
        .map(str::to_string);
        assert_eq!(rainy.as_deref(), Some("/backgrounds/rainy-night.png"));

        let clear = resolve_image(
            &set,
            &document(&[("环境/时间", &["20"]), ("环境/天气", &["晴"])]).condition_fields(),
            &comparators(),
        )
        .expect("the combination resolves")
        .map(str::to_string);
        assert_eq!(
            clear.as_deref(),
            Some("/backgrounds/day.png"),
            "one part short falls through to the fallback"
        );
    }

    #[test]
    fn a_condition_that_combines_and_compares_is_refused_with_a_reason() {
        let config = panel_with_background(vec![candidate(
            "/backgrounds/a.png",
            Some(ConditionSpec {
                source: SOURCE_FIELD.to_string(),
                field: Some("关系/好感".to_string()),
                op: "gte".to_string(),
                value: Some("30".to_string()),
                compose: Some(ConditionCompose::All(vec![number_at_least("关系/好感", "10")])),
                ..Default::default()
            }),
        )]);

        let errors = validate_panel_config(&config, &comparators(), &[]);

        assert_eq!(
            errors,
            vec![StatePanelError::InvalidCondition {
                source: "/backgrounds/a.png".to_string(),
                reason: "a condition cannot both combine parts and compare one field; drop either the parts or the comparison".to_string(),
            }]
        );
    }

    #[test]
    fn a_shared_module_name_must_be_one_an_entry_script_can_import() {
        let mut config = panel_with_background(vec![candidate("/backgrounds/a.png", None)]);

        for name in [
            "scene",
            "scripts/scene.js",
            "scene.js/../x.js",
            ".js",
            "sc ene.js",
        ] {
            config.scripts = BTreeMap::from([(
                name.to_string(),
                "export default () => ({});".to_string(),
            )]);
            let errors = validate_panel_config(&config, &comparators(), &[]);

            assert!(
                matches!(errors.as_slice(), [StatePanelError::InvalidScriptModule { .. }]),
                "`{name}` must be refused, got {errors:?}"
            );
        }

        // A module with no source yet is a work in progress, not a broken scene:
        // the add-then-paste flow saves in between.
        config.scripts = BTreeMap::from([("scene.js".to_string(), "   ".to_string())]);
        assert!(validate_panel_config(&config, &comparators(), &[]).is_empty());

        config.scripts = BTreeMap::from([(
            "scene-2.js".to_string(),
            "export default () => ({});".to_string(),
        )]);
        assert!(validate_panel_config(&config, &comparators(), &[]).is_empty());
    }

    #[test]
    fn a_picture_set_that_declares_an_empty_script_is_refused_at_save_time() {
        let mut config = panel_with_background(vec![candidate("/backgrounds/a.png", None)]);
        if let Some(set) = config.panels[0].background.as_mut() {
            set.condition_script = Some(ScriptSpec {
                script: "   ".to_string(),
                entry: None,
            });
        }

        let errors = validate_panel_config(&config, &comparators(), &[]);

        assert_eq!(
            errors,
            vec![StatePanelError::EmptyConditionScript {
                location: "panel `环境` background".to_string(),
            }]
        );
    }

    #[test]
    fn a_theme_stylesheet_is_checked_mechanically() {
        let mut config = panel_with_background(vec![candidate("/backgrounds/a.png", None)]);

        // Ordinary rules, a container at-rule, and host-path pictures — quoted or
        // bare — are what a theme is made of.
        config.css = "/* night */\n\
             .tt-state-root .tt-state-field { color: #fff }\n\
             @media (max-width: 600px) { .tt-state-root .tt-state-rail { display: none } }\n\
             .tt-state-root .tt-state-panel__body { background-image: url(/backgrounds/night.png) }\n\
             .tt-state-root .tt-state-field__value { background-image: url(\"/user/images/a.png\") }"
            .to_string();
        assert!(validate_panel_config(&config, &comparators(), &[]).is_empty());

        // An empty stylesheet is "not configured", which is not a problem.
        config.css = "   ".to_string();
        assert!(validate_panel_config(&config, &comparators(), &[]).is_empty());

        // One stylesheet can break several rules at once — an `@import` of an
        // external stylesheet breaks two — so the check is that every complaint
        // is about the theme, never that there is exactly one.
        let refused = |css: &str| {
            let mut config = config.clone();
            config.css = css.to_string();
            let errors = validate_panel_config(&config, &comparators(), &[]);
            assert!(
                !errors.is_empty()
                    && errors
                        .iter()
                        .all(|error| matches!(error, StatePanelError::InvalidThemeCss { .. })),
                "`{css}` must be refused, got {errors:?}"
            );
        };

        for css in [
            "@import url(https://example.test/x.css);",
            ".tt-state-root .a { background: url(https://example.test/x.png) }",
            ".tt-state-root .a { background: url(/backgrounds/../secret.png) }",
            ".tt-state-root .a { background: url(//example.test/x.png) }",
            ".tt-state-root .a { background: url() }",
            ".tt-state-root .a { content: '</style><script>alert(1)</script>' }",
        ] {
            refused(css);
        }

        refused(&"a".repeat(MAX_THEME_CSS_CHARS + 1));
    }

    #[test]
    fn a_prose_path_must_live_in_the_persistent_content() {
        let ok = super::StateProseSpec {
            path: "persist/心声.md".to_string(),
            title: "心声".to_string(),
        };
        assert!(super::validate_prose(&ok).is_empty());

        // Only the versioned directory can be read from a floor's own version: a
        // run-scoped path would be read from wherever the reader happened to be.
        for refused in ["", "persist/", "output/心声.md", "persist/../output/心声.md"] {
            let spec = super::StateProseSpec {
                path: refused.to_string(),
                title: String::new(),
            };
            assert!(
                !super::validate_prose(&spec).is_empty(),
                "`{refused}` must be refused"
            );
        }
    }
}
