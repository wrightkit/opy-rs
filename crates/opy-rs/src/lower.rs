use std::collections::{HashMap, HashSet};

use crate::hir::types::{
    Annotation as HirAnnotation, AnnotationArg as HirAnnotationArg, Declaration, Define,
    DictEntry as HirDictEntry, Event, Expr as HirExpr, Generator, IfBranch, PROTOCOL_VERSION,
    Position, PreprocessingState, Program as HirProgram, Protocol, Rule, RuleEntry,
    Settings as HirSettings, SettingsNode as HirSettingsNode, SourceFile, Span as HirSpan,
    Stmt as HirStmt, SwitchArm as HirSwitchArm, default_var_index,
};

use crate::cst::{self, CallArg, Decl, Expr, RuleEntry as CstRuleEntry, Stmt, TopLevel};
use crate::diag::{OpyError, OpyResult, Span};
use crate::manifest::{Function, FunctionKind, Manifest, Param, ParamDefault};
use workshop_rs::catalog::Catalog;

const PROTOCOL_NAME: &str = "wright/opy-hir";

#[derive(Clone, Copy, PartialEq, Eq)]
enum CallPosition {
    Statement,
    Value,
    ForIterable,
    LambdaArgument,
    MacroBody,
}

#[derive(Debug, Clone)]
enum EnumMemberValue {
    Number(f64),
    Expr(Expr),
}

#[derive(Debug, Clone)]
struct EnumMember {
    name: String,
    value: EnumMemberValue,
}

/// The auto-increment state of a custom enum while its members are
/// collected. The reference tracks `lastIntValue` the same way: `= n`
/// reseeds the counter at `n + 1`, `= -n` yields `-n + 1` for the next
/// bare member, and any other explicit value breaks the numeric chain.
#[derive(Debug, Clone, Copy)]
enum EnumCounter {
    Number(f64),
    /// `= -<number>` keeps the negated literal so the next bare member
    /// resolves to `-n + 1`.
    Negated(f64),
    /// The previous member had a non-numeric explicit value; the next
    /// bare member is a hard error.
    Other,
}

struct Lowerer {
    global_declarations: HashMap<String, usize>,
    player_declarations: HashMap<String, usize>,
    subroutine_declarations: HashMap<String, usize>,
    subroutine_definitions: Vec<(String, usize)>,
    constant_declarations: HashMap<String, usize>,
    macro_declarations: HashMap<String, usize>,
    enums: HashMap<String, Vec<EnumMember>>,
    enum_declarations: HashMap<String, usize>,
    locals: Vec<String>,
    current_order: usize,
    allow_dict_literal: bool,
    manifest: &'static Manifest,
    catalog: &'static Catalog,
    texture_used: bool,
    setup_tags: bool,
    first_error: Option<OpyError>,
}

impl Lowerer {
    fn new(manifest: &'static Manifest, catalog: &'static Catalog) -> Self {
        Self {
            global_declarations: HashMap::new(),
            player_declarations: HashMap::new(),
            subroutine_declarations: HashMap::new(),
            subroutine_definitions: Vec::new(),
            constant_declarations: HashMap::new(),
            macro_declarations: HashMap::new(),
            enums: HashMap::new(),
            enum_declarations: HashMap::new(),
            locals: Vec::new(),
            current_order: 0,
            allow_dict_literal: false,
            manifest,
            catalog,
            texture_used: false,
            setup_tags: false,
            first_error: None,
        }
    }
}

mod declarations;
mod expressions;
pub(crate) mod policy;
pub(crate) mod special_forms;
mod statements;
mod textures;

pub(crate) fn compressed_component_mode(values: &[Vec<f64>]) -> Option<(bool, f64)> {
    let component_count = values.first()?.len();
    let is_vector = component_count == 3;
    (matches!(component_count, 1 | 3) && values.iter().all(|value| value.len() == component_count))
        .then_some((is_vector, if is_vector { 4999.0 } else { 49999.0 }))
}

pub fn lower(
    program: &cst::Program,
    files: Vec<SourceFile>,
    defines: Vec<Define>,
) -> OpyResult<HirProgram> {
    lower_with_preprocessing(program, files, defines, &PreprocessingState::default())
}

pub fn lower_with_preprocessing(
    program: &cst::Program,
    files: Vec<SourceFile>,
    defines: Vec<Define>,
    preprocessing: &PreprocessingState,
) -> OpyResult<HirProgram> {
    let manifest = match Manifest::builtin() {
        Ok(manifest) => manifest,
        Err(error) => {
            return Err(OpyError::new(
                "manifest-error",
                format!("cannot load the OPY semantic compatibility manifest: {error}"),
            ));
        }
    };
    let catalog = Catalog::builtin_ref().map_err(|error| {
        OpyError::new(
            "catalog-error",
            format!("cannot load the Workshop catalog: {error}"),
        )
    })?;
    lower_program(program, files, defines, preprocessing, manifest, catalog)
}

fn lower_program(
    program: &cst::Program,
    files: Vec<SourceFile>,
    defines: Vec<Define>,
    preprocessing: &PreprocessingState,
    manifest: &'static Manifest,
    catalog: &'static Catalog,
) -> OpyResult<HirProgram> {
    let mut lowerer = Lowerer::new(manifest, catalog);
    lowerer.setup_tags = preprocessing
        .directives
        .iter()
        .any(|directive| matches!(directive.name.as_str(), "setupTags" | "setupTx"));
    lowerer.collect_symbols(program);

    let mut declarations = Vec::new();
    let mut rules = Vec::new();
    let mut implicit_subroutines = HashSet::new();
    for (order, item) in program.top_level.iter().enumerate() {
        lowerer.current_order = order;
        match item {
            TopLevel::Declaration(decl) => {
                if let Some(declaration) = lowerer.lower_declaration(decl) {
                    declarations.push(declaration);
                }
            }
            TopLevel::Rule(CstRuleEntry::Rule(rule)) => rules.push(RuleEntry::Rule(
                lowerer.lower_rule(rule, files.as_slice(), preprocessing)?,
            )),
            TopLevel::Rule(CstRuleEntry::SubroutineDef {
                name,
                presentation_name,
                span,
                name_span,
                body,
                annotations,
                rule_prefix,
            }) => {
                if !lowerer.subroutine_declarations.contains_key(name)
                    && implicit_subroutines.insert(name.clone())
                {
                    declarations.push(Declaration::Subroutine {
                        name: name.clone(),
                        index: None,
                        span: Some(span.into()),
                        name_span: Some(name_span.into()),
                    });
                }
                let base_name = presentation_name
                    .as_deref()
                    .map(str::to_string)
                    .unwrap_or_else(|| name.clone());
                let generated_name = render_rule_name(
                    &base_name,
                    rule_prefix.as_deref(),
                    false,
                    *span,
                    files.as_slice(),
                    preprocessing,
                )?;
                rules.push(RuleEntry::SubroutineDef {
                    kind: "subroutineDef".to_string(),
                    name: generated_name,
                    source_name: name.clone(),
                    span: Some(span.into()),
                    name_span: Some(name_span.into()),
                    body: lowerer.lower_block(body, &[], false, true, false),
                    annotations: lower_annotations(annotations),
                });
            }
        }
    }

    if let Some(error) = lowerer.first_error.take() {
        return Err(error);
    }

    if lowerer.texture_used || lowerer.setup_tags {
        declarations.insert(
            0,
            Declaration::GlobalVariable {
                name: "__holygrail__".to_string(),
                index: Some(127),
                span: None,
                name_span: None,
                initializer: None,
            },
        );
        rules.insert(0, texture_setup_rule());
    }

    Ok(HirProgram {
        protocol: Protocol {
            name: PROTOCOL_NAME.to_string(),
            version: PROTOCOL_VERSION.to_string(),
        },
        generator: Generator {
            name: crate::LANGUAGE_NAME.to_string(),
            version: crate::LANGUAGE_VERSION.to_string(),
            frontend: crate::LANGUAGE_NAME.to_string(),
        },
        files,
        defines,
        declarations,
        rules,
        settings: program.settings.as_ref().map(lower_settings),
        preprocessing: preprocessing.clone(),
    })
}

fn texture_setup_rule() -> RuleEntry {
    RuleEntry::Rule(Rule {
        name: format!("OverPy <{}tx> / <{}fg> setup code", '\u{00ad}', '\u{00ad}'),
        span: None,
        name_span: None,
        disabled: false,
        delimiter: false,
        new_page: None,
        annotations: Vec::new(),
        event: Event {
            name: "global".to_string(),
            args: Vec::new(),
            span: None,
        },
        conditions: Vec::new(),
        actions: vec![
            synthetic_statement(synthetic_call(
                "createDummy",
                vec![
                    synthetic_call("getAllHeroes", Vec::new()),
                    texture_dummy_team(),
                    synthetic_boolean(false),
                    HirExpr::Null { span: None },
                    HirExpr::Null { span: None },
                ],
            )),
            synthetic_statement(synthetic_method(
                texture_dummy_player(),
                "startForcingName",
                vec![texture_marker('\u{303c}')],
            )),
            texture_state_assignment(synthetic_method(
                texture_dummy_first_value(),
                "split",
                vec![HirExpr::Array {
                    elements: Vec::new(),
                    span: None,
                }],
            )),
            synthetic_statement(synthetic_method(
                texture_dummy_player(),
                "startForcingName",
                vec![texture_marker('\u{840}')],
            )),
            texture_state_assignment(synthetic_method(
                synthetic_method(
                    texture_marker('\u{303c}'),
                    "replace",
                    vec![texture_state_variable(), texture_dummy_first_value()],
                ),
                "substring",
                vec![synthetic_number(126.0), synthetic_boolean(true)],
            )),
            synthetic_statement(synthetic_call("destroyAllDummies", Vec::new())),
        ],
    })
}

fn synthetic_call(name: &str, args: Vec<HirExpr>) -> HirExpr {
    HirExpr::Call {
        name: name.to_string(),
        args,
        debug_source: None,
        span: None,
    }
}

fn synthetic_method(receiver: HirExpr, name: &str, args: Vec<HirExpr>) -> HirExpr {
    HirExpr::ReceiverCall {
        receiver: Box::new(receiver),
        name: name.to_string(),
        args,
        span: None,
    }
}

fn synthetic_enum(value_type: &str, value: &str) -> HirExpr {
    HirExpr::Enum {
        value_type: value_type.to_string(),
        value: value.to_string(),
        literal: false,
        span: None,
    }
}

fn synthetic_number(value: f64) -> HirExpr {
    HirExpr::Number {
        value,
        text: value.to_string(),
        span: None,
    }
}

fn synthetic_boolean(value: bool) -> HirExpr {
    HirExpr::Bool { value, span: None }
}

fn synthetic_index(array: HirExpr, index: HirExpr) -> HirExpr {
    HirExpr::Index {
        array: Box::new(array),
        index: Box::new(index),
        span: None,
    }
}

fn synthetic_conditional(then_value: HirExpr, condition: HirExpr, else_value: HirExpr) -> HirExpr {
    HirExpr::Conditional {
        then_value: Box::new(then_value),
        condition: Box::new(condition),
        else_value: Box::new(else_value),
        span: None,
    }
}

fn synthetic_statement(expr: HirExpr) -> HirStmt {
    HirStmt::Expr {
        expr: Box::new(expr),
        span: None,
    }
}

fn texture_state_variable() -> HirExpr {
    HirExpr::GlobalVar {
        name: "__holygrail__".to_string(),
        span: None,
    }
}

fn texture_state_assignment(value: HirExpr) -> HirStmt {
    HirStmt::Assign {
        target: Box::new(texture_state_variable()),
        value: Box::new(value),
        span: None,
    }
}

fn texture_dummy_player() -> HirExpr {
    synthetic_call("getLastCreatedEntity", Vec::new())
}

fn texture_dummy_first_value() -> HirExpr {
    synthetic_index(texture_dummy_player(), synthetic_number(0.0))
}

fn texture_dummy_team() -> HirExpr {
    let team_1 = synthetic_enum("Team", "TEAM_1");
    let team_2 = synthetic_enum("Team", "TEAM_2");
    synthetic_conditional(
        team_1.clone(),
        synthetic_call("getNumberOfSlots", vec![team_1]),
        synthetic_conditional(
            team_2.clone(),
            synthetic_call("getNumberOfSlots", vec![team_2]),
            synthetic_boolean(true),
        ),
    )
}

fn texture_marker(suffix: char) -> HirExpr {
    HirExpr::String {
        value: format!("{}{}", "_".repeat(126), suffix),
        span: None,
    }
}

pub(crate) struct SettingsLowerer {
    lowerer: Lowerer,
}

impl SettingsLowerer {
    pub(crate) fn new(program: &cst::Program) -> OpyResult<Self> {
        let manifest = Manifest::builtin().map_err(|error| {
            OpyError::new(
                "manifest-error",
                format!("cannot load the OPY semantic compatibility manifest: {error}"),
            )
        })?;
        let catalog = Catalog::builtin_ref().map_err(|error| {
            OpyError::new(
                "catalog-error",
                format!("cannot load the Workshop catalog: {error}"),
            )
        })?;
        let mut lowerer = Lowerer::new(manifest, catalog);
        lowerer.current_order = program.top_level.len();
        lowerer.allow_dict_literal = true;
        // Bare `Infinity` resolves as a constant so settings evaluation can
        // fold it like the pinned JavaScript global (#512); `NaN`, `null`,
        // and `undefined` stay unresolved as they are upstream.
        lowerer
            .constant_declarations
            .insert("Infinity".to_string(), 0);
        lowerer.collect_symbols(program);
        Ok(Self { lowerer })
    }

    pub(crate) fn lower(
        &mut self,
        text: &str,
        file: u32,
        origin: crate::diag::Position,
    ) -> OpyResult<HirExpr> {
        let expression = crate::parser::parse_expression_fragment(text, file, origin, None, &[])?;
        self.lowerer.first_error = None;
        self.lowerer.texture_used = false;
        let lowered = self
            .lowerer
            .lower_expr(&expression, &[], CallPosition::Value);
        self.lowerer.first_error.take().map_or(Ok(lowered), Err)
    }
}

fn prefixed_rule_name(name: &str, prefix: Option<&str>, delimiter: bool) -> String {
    match prefix {
        Some(prefix) if !prefix.is_empty() && !delimiter && !name.is_empty() => {
            format!("[{prefix}] {name}")
        }
        _ => name.to_string(),
    }
}

pub(crate) fn strip_rule_name_formatting(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars().filter(|character| {
        !matches!(
            character,
            '\u{200B}' | '\u{200E}' | '\u{200F}' | '\u{FEFF}' | '\u{061C}'
        )
    })
}

#[derive(Clone, Debug)]
enum TemplateValue {
    String(String),
    Bool(bool),
}

fn rule_template_values(
    name: &str,
    prefix: &str,
    file: &str,
    path: &str,
    delimiter: bool,
) -> [(&'static str, TemplateValue); 14] {
    [
        ("$rule", TemplateValue::String(name.to_string())),
        ("$prefix", TemplateValue::String(prefix.to_string())),
        ("$file", TemplateValue::String(file.to_string())),
        ("$path", TemplateValue::String(path.to_string())),
        ("$isDelimiter", TemplateValue::Bool(delimiter)),
        ("$prefixTitle", TemplateValue::String(title_case(prefix))),
        ("$prefixUpper", TemplateValue::String(prefix.to_uppercase())),
        ("$prefixLower", TemplateValue::String(prefix.to_lowercase())),
        ("$fileTitle", TemplateValue::String(title_case(file))),
        ("$fileUpper", TemplateValue::String(file.to_uppercase())),
        ("$fileLower", TemplateValue::String(file.to_lowercase())),
        ("$pathTitle", TemplateValue::String(title_case(path))),
        ("$pathUpper", TemplateValue::String(path.to_uppercase())),
        ("$pathLower", TemplateValue::String(path.to_lowercase())),
    ]
}

fn render_rule_name(
    name: &str,
    prefix: Option<&str>,
    delimiter: bool,
    span: Span,
    files: &[SourceFile],
    preprocessing: &PreprocessingState,
) -> OpyResult<String> {
    let Some(template) = preprocessing
        .rule_prefix_template
        .as_ref()
        .map(|value| value.value.as_str())
    else {
        return Ok(
            strip_rule_name_formatting(&prefixed_rule_name(name, prefix, delimiter)).collect(),
        );
    };
    let (file, path) = rule_file_parts(span.file, files);
    let prefix = prefix.unwrap_or_default();
    let values = rule_template_values(name, prefix, &file, &path, delimiter);
    evaluate_template(template, &values)
        .map(|name| strip_rule_name_formatting(&name).collect())
        .map_err(|message| {
            OpyError::at(
                "rule-prefix-template-invalid",
                format!("could not resolve rule prefix template: {message}"),
                span,
            )
        })
}

pub(crate) fn render_generated_rule_name(name: &str, preprocessing: &PreprocessingState) -> String {
    let Some(template) = preprocessing
        .rule_prefix_template
        .as_ref()
        .map(|value| value.value.as_str())
    else {
        return name.to_string();
    };
    let values = rule_template_values(name, "", "", "", false);
    evaluate_template(template, &values)
        .map(|rendered| strip_rule_name_formatting(&rendered).collect())
        .unwrap_or_else(|_| name.to_string())
}

fn rule_file_parts(file_id: u32, files: &[SourceFile]) -> (String, String) {
    let path = files
        .iter()
        .find(|file| file.id == file_id)
        .map(|file| file.path.replace('\\', "/"))
        .unwrap_or_default();
    let without_extension = path
        .strip_suffix(".opy")
        .or_else(|| path.strip_suffix(".OPY"))
        .unwrap_or(&path)
        .to_string();
    let file = without_extension
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string();
    (file, without_extension)
}

fn title_case(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut capitalize = true;
    for ch in value.chars() {
        if ch == '_' {
            result.push(' ');
            capitalize = true;
        } else if capitalize && ch.is_ascii_alphabetic() {
            result.push(ch.to_ascii_uppercase());
            capitalize = false;
        } else {
            result.push(ch);
            if !ch.is_whitespace() && ch != '/' {
                capitalize = false;
            }
        }
        if ch == '/' || ch.is_whitespace() {
            capitalize = true;
        }
    }
    result
}

fn evaluate_template(template: &str, values: &[(&str, TemplateValue)]) -> Result<String, String> {
    if let Some((then_value, condition, else_value)) = split_conditional(template) {
        let branch = if evaluate_condition(condition, values)? {
            then_value
        } else {
            else_value
        };
        return evaluate_string(branch, values);
    }
    evaluate_string(template, values)
}

fn split_conditional(value: &str) -> Option<(&str, &str, &str)> {
    let mut quote = None;
    let mut depth = 0usize;
    let mut if_start = None;
    let mut else_start = None;
    for (index, ch) in value.char_indices() {
        match (ch, quote) {
            ('"' | '\'', None) => quote = Some(ch),
            (ch, Some(current)) if ch == current => quote = None,
            ('{', None) => depth += 1,
            ('}', None) => depth = depth.saturating_sub(1),
            _ => {}
        }
        if quote.is_none() && depth == 0 {
            if value[index..].starts_with(" if ") && if_start.is_none() {
                if_start = Some(index);
            } else if value[index..].starts_with(" else ") && else_start.is_none() {
                else_start = Some(index);
            }
        }
    }
    let (Some(if_start), Some(else_start)) = (if_start, else_start) else {
        return None;
    };
    Some((
        value[..if_start].trim(),
        value[if_start + 4..else_start].trim(),
        value[else_start + 6..].trim(),
    ))
}

fn evaluate_condition(value: &str, values: &[(&str, TemplateValue)]) -> Result<bool, String> {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix("not ") {
        return Ok(!evaluate_condition(rest, values)?);
    }
    if let Some((left, right)) = value.split_once(" or ") {
        return Ok(evaluate_condition(left, values)? || evaluate_condition(right, values)?);
    }
    if let Some((left, right)) = value.split_once(" and ") {
        return Ok(evaluate_condition(left, values)? && evaluate_condition(right, values)?);
    }
    match lookup_template_value(value, values)? {
        TemplateValue::Bool(value) => Ok(value),
        TemplateValue::String(value) => Ok(!value.is_empty()),
    }
}

fn evaluate_string(value: &str, values: &[(&str, TemplateValue)]) -> Result<String, String> {
    let value = value.trim();
    if let Some(body) = value
        .strip_prefix("f\"")
        .and_then(|body| body.strip_suffix('"'))
    {
        return interpolate_fstring(body, values);
    }
    if let Some(body) = value
        .strip_prefix("f'")
        .and_then(|body| body.strip_suffix('\''))
    {
        return interpolate_fstring(body, values);
    }
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        return Ok(value[1..value.len() - 1].to_string());
    }
    match lookup_template_value(value, values)? {
        TemplateValue::String(value) => Ok(value),
        TemplateValue::Bool(value) => Ok(value.to_string()),
    }
}

fn interpolate_fstring(body: &str, values: &[(&str, TemplateValue)]) -> Result<String, String> {
    let mut result = String::new();
    let mut remaining = body;
    while let Some(start) = remaining.find('{') {
        result.push_str(&remaining[..start]);
        let end = remaining[start + 1..]
            .find('}')
            .ok_or_else(|| "unterminated interpolation".to_string())?
            + start
            + 1;
        result.push_str(&evaluate_string(&remaining[start + 1..end], values)?);
        remaining = &remaining[end + 1..];
    }
    result.push_str(remaining);
    Ok(result)
}

fn lookup_template_value(
    value: &str,
    values: &[(&str, TemplateValue)],
) -> Result<TemplateValue, String> {
    let value = value.trim();
    let (base, mut methods) = value
        .split_once('.')
        .map_or((value, ""), |(base, methods)| (base, methods));
    let mut result = values
        .iter()
        .find(|(name, _)| *name == base)
        .map(|(_, value)| value.clone())
        .ok_or_else(|| format!("unsupported expression '{value}'"))?;
    while !methods.is_empty() {
        let (method, rest) = methods
            .split_once('.')
            .map_or((methods, ""), |(method, rest)| (method, rest));
        if method == "upper()" {
            result = TemplateValue::String(as_string(&result).to_uppercase());
        } else if method == "lower()" {
            result = TemplateValue::String(as_string(&result).to_lowercase());
        } else if let Some(args) = method
            .strip_prefix("replace(")
            .and_then(|v| v.strip_suffix(')'))
        {
            let (from, to) = args
                .split_once(',')
                .ok_or_else(|| "replace expects two arguments".to_string())?;
            let from = unquote_template_arg(from.trim())?;
            let to = unquote_template_arg(to.trim())?;
            result = TemplateValue::String(as_string(&result).replace(&from, &to));
        } else {
            return Err(format!("unsupported method '{method}'"));
        }
        methods = rest;
    }
    Ok(result)
}

fn as_string(value: &TemplateValue) -> String {
    match value {
        TemplateValue::String(value) => value.clone(),
        TemplateValue::Bool(value) => value.to_string(),
    }
}

fn unquote_template_arg(value: &str) -> Result<String, String> {
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        Ok(value[1..value.len() - 1].to_string())
    } else {
        Err(format!("expected a quoted string argument, got '{value}'"))
    }
}

fn lower_annotations(annotations: &[cst::Annotation]) -> Vec<HirAnnotation> {
    annotations
        .iter()
        .map(|annotation| HirAnnotation {
            name: annotation.name.clone(),
            args: annotation
                .args
                .iter()
                .map(|arg| HirAnnotationArg {
                    text: arg.text.clone(),
                    span: Some(arg.span.into()),
                })
                .collect(),
            span: Some(annotation.span.into()),
        })
        .collect()
}

fn lower_settings(settings: &cst::Settings) -> HirSettings {
    HirSettings {
        span: Some(settings.span.into()),
        children: settings.children.iter().map(lower_settings_node).collect(),
    }
}

fn lower_settings_node(node: &cst::SettingsNode) -> HirSettingsNode {
    match node {
        cst::SettingsNode::Group {
            name,
            children,
            span,
        } => HirSettingsNode::Group {
            name: name.clone(),
            children: children.iter().map(lower_settings_node).collect(),
            span: Some((*span).into()),
        },
        cst::SettingsNode::Number { name, value, span } => HirSettingsNode::Number {
            name: name.clone(),
            value: *value,
            span: Some((*span).into()),
        },
        cst::SettingsNode::Bool { name, value, span } => HirSettingsNode::Bool {
            name: name.clone(),
            value: *value,
            span: Some((*span).into()),
        },
        cst::SettingsNode::String { name, value, span } => HirSettingsNode::String {
            name: name.clone(),
            value: value.clone(),
            span: Some((*span).into()),
        },
        cst::SettingsNode::Raw { name, value, span } => HirSettingsNode::Raw {
            name: name.clone(),
            value: value.clone(),
            span: Some((*span).into()),
        },
        cst::SettingsNode::List {
            name,
            elements,
            span,
        } => HirSettingsNode::List {
            name: name.clone(),
            elements: elements
                .iter()
                .map(|element| crate::hir::types::SettingsListElement {
                    value: element.value.clone(),
                    evaluated: (!element.expr).then(|| element.value.clone()),
                    span: Some(element.span.into()),
                })
                .collect(),
            span: Some((*span).into()),
        },
    }
}

impl Lowerer {
    fn error_at(&mut self, code: &str, message: String, span: Span) {
        if self.first_error.is_none() {
            self.first_error = Some(OpyError::at(code, message, span));
        }
    }

    fn error_at_candidates(
        &mut self,
        code: &str,
        message: String,
        span: Span,
        rejected: &str,
        pool: &[crate::matcher::MatchCandidate],
    ) {
        let ranked = crate::matcher::rank(rejected, pool);
        self.error_at(code, crate::matcher::did_you_mean(message, &ranked), span);
    }

    fn error_at_closed_candidates(
        &mut self,
        code: &str,
        message: String,
        span: Span,
        rejected: &str,
        pool: &[crate::matcher::MatchCandidate],
    ) {
        let ranked = crate::matcher::rank_closed(rejected, pool);
        self.error_at(code, crate::matcher::did_you_mean(message, &ranked), span);
    }
}

fn keyword_spellings(param: &Param) -> Vec<String> {
    let mut spellings = vec![param.name.clone()];
    spellings.extend(param.alternate_names.iter().cloned());
    spellings
}

fn arg_span(args: &[CallArg], fallback: Span) -> Span {
    args.first().map(CallArg::span).unwrap_or(fallback)
}

fn context_player_expr(name: &str, span: Option<Span>) -> Option<HirExpr> {
    match name {
        "eventPlayer" => Some(HirExpr::EventPlayer {
            span: span.map(Into::into),
        }),
        "localPlayer" | "attacker" | "victim" | "healer" | "healee" => Some(HirExpr::Call {
            name: name.to_string(),
            args: Vec::new(),
            debug_source: None,
            span: span.map(Into::into),
        }),
        "hostPlayer" => Some(HirExpr::HostPlayer {
            span: span.map(Into::into),
        }),
        _ => None,
    }
}

/// The bare-spelled values a call with an empty argument list resolves to:
/// upstream `funcKw` entries with `args === null` produce the same AST
/// whether written bare or as `name()` (#529). These are the context
/// players and the `true`/`false`/`null` literals; the remaining
/// bare-spelled names (`healer`, `healee`, the `eventDamage` family, and
/// `ruleCondition`) are manifest functions and resolve through the
/// manifest path instead. `RULE_CONDITION` is a constant spelling, not a
/// `funcKw` name, so `RULE_CONDITION()` stays rejected like the reference.
fn bare_spelled_value_expr(name: &str, span: Option<Span>) -> Option<HirExpr> {
    if let Some(player) = context_player_expr(name, span) {
        return Some(player);
    }
    match name {
        "true" => Some(HirExpr::Bool {
            value: true,
            span: span.map(Into::into),
        }),
        "false" => Some(HirExpr::Bool {
            value: false,
            span: span.map(Into::into),
        }),
        "null" => Some(HirExpr::Null {
            span: span.map(Into::into),
        }),
        _ => None,
    }
}

/// The declaration namespace an identifier would occupy: `globalvar`,
/// `playervar`, or `subroutine`/`def`. The pinned upstream compiler reserves
/// a different name set per namespace, so a name refused for a `globalvar`
/// can still be a legal `playervar` name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameNamespace {
    /// `globalvar` declarations — upstream `reservedNames`.
    Global,
    /// `playervar` declarations — upstream `reservedMemberNames`.
    Player,
    /// `subroutine`/`def` declarations — upstream `reservedSubroutineNames`.
    Subroutine,
}

/// Whether the pinned upstream compiler (OverPy 9.7.10) reserves `name` for
/// `namespace` declarations — such a declaration is never valid source.
///
/// Mirrors the upstream reservation rules: globals reserve the grammar
/// keywords, the type lattice (`typeMatrix`), `AsyncBehavior`, and the
/// bare-spelled builtins (`funcKw` entries with `args === null`: context
/// names, literals, `ruleCondition`, …); subroutines reserve the keywords
/// plus builtins callable with an empty argument list (`args.length === 0`,
/// mirrored here by manifest functions that declare no parameters); player
/// variables reserve only the member axis names `x`, `y`, `z`.
pub fn is_reserved_name(name: &str, namespace: NameNamespace) -> bool {
    match namespace {
        NameNamespace::Player => matches!(name, "x" | "y" | "z"),
        NameNamespace::Global => {
            RESERVED_KEYWORDS.contains(&name) || RESERVED_GLOBAL_NAMES.contains(&name)
        }
        NameNamespace::Subroutine => {
            RESERVED_KEYWORDS.contains(&name)
                || Manifest::builtin().ok().is_some_and(|manifest| {
                    manifest
                        .resolve_function(name)
                        .is_some_and(|function| function.params.is_empty())
                })
        }
    }
}

const RESERVED_KEYWORDS: &[&str] = &[
    "and",
    "bool",
    "case",
    "def",
    "default",
    "del",
    "elif",
    "else",
    "enum",
    "float",
    "for",
    "globalvar",
    "goto",
    "if",
    "in",
    "int",
    "lambda",
    "loc",
    "macro",
    "not",
    "or",
    "playervar",
    "rule",
    "self",
    "settings",
    "signed",
    "subroutine",
    "switch",
    "unsigned",
    "while",
];

const RESERVED_GLOBAL_NAMES: &[&str] = &[
    "AccelReeval",
    "Array",
    "AssistId",
    "AssistReeval",
    "AsyncBehavior",
    "BarrierLos",
    "Beam",
    "BoolLiteral",
    "Button",
    "ButtonLiteral",
    "ChaseRateReeval",
    "ChaseTimeReeval",
    "Clip",
    "Color",
    "ColorLiteral",
    "Comms",
    "CustomStringLiteral",
    "DamageModificationId",
    "DamageReeval",
    "Dict",
    "DictElem",
    "DictKey",
    "Direction",
    "DotId",
    "DynamicEffect",
    "Effect",
    "EffectReeval",
    "EntityId",
    "FacingReeval",
    "FloatLiteral",
    "Gamemode",
    "GamemodeLiteral",
    "GlobalVariable",
    "HealingModificationId",
    "HealingReeval",
    "Health",
    "HealthPoolId",
    "Hero",
    "HeroLiteral",
    "HeroStat",
    "HotId",
    "HudPosition",
    "HudReeval",
    "Icon",
    "IconReeval",
    "Impulse",
    "IntLiteral",
    "Invis",
    "Label",
    "Lambda",
    "LocalizedStringLiteral",
    "LosCheck",
    "Map",
    "MapLiteral",
    "ModifyHealth",
    "Object",
    "OutlineVisibility",
    "Player",
    "PlayerVariable",
    "Position",
    "ProgressHudReeval",
    "ProgressWorldTextReeval",
    "Projectile",
    "ProjectileEffectReeval",
    "Raycast",
    "Relativity",
    "SignedFloatLiteral",
    "SignedIntLiteral",
    "SpecVisibility",
    "StartRuleBehavior",
    "Stat",
    "Status",
    "String",
    "StringLiteral",
    "Subroutine",
    "Team",
    "TeamLiteral",
    "TextId",
    "Throttle",
    "ThrottleReeval",
    "Transform",
    "Type",
    "UnsignedFloatLiteral",
    "UnsignedIntLiteral",
    "Value",
    "Vector",
    "Velocity",
    "Wait",
    "WorldTextReeval",
    "__ChaseReeval__",
    "__Operation__",
    "__Operator__",
    "__Rounding__",
    "__global__",
    "attacker",
    "break",
    "continue",
    "eventAbility",
    "eventDamage",
    "eventDirection",
    "eventHealing",
    "eventPlayer",
    "eventWasCriticalHit",
    "eventWasEnvironment",
    "eventWasHealthPack",
    "false",
    "healee",
    "healer",
    "hostPlayer",
    "localPlayer",
    "null",
    "pass",
    "return",
    "ruleCondition",
    "true",
    "victim",
    "void",
];

fn assignable_receiver(receiver: &Expr) -> bool {
    match receiver {
        Expr::Name { name, .. } => !matches!(
            name.as_str(),
            "eventPlayer" | "hostPlayer" | "attacker" | "victim"
        ),
        Expr::Array { .. } | Expr::Index { .. } | Expr::Member { .. } => true,
        _ => false,
    }
}

fn expr_identifier(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name { name, .. } => Some(name.as_str()),
        _ => None,
    }
}

fn indexed_expr_depth(expr: &Expr) -> usize {
    match expr {
        Expr::Index { array, .. } => 1 + indexed_expr_depth(array),
        _ => 0,
    }
}

impl From<Span> for HirSpan {
    fn from(span: Span) -> HirSpan {
        HirSpan {
            file: span.file,
            start: Position {
                line: span.start.line,
                col: span.start.col,
            },
            end: Position {
                line: span.end.line,
                col: span.end.col,
            },
        }
    }
}

impl From<&Span> for HirSpan {
    fn from(span: &Span) -> HirSpan {
        (*span).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::types::{Expr as HirExpr, RuleEntry as HirRuleEntry, Stmt as HirStmt};
    use crate::lexer::{LexInput, lex};
    use crate::parser::parse;

    fn lower_ok(text: &str) -> HirProgram {
        let tokens = lex(LexInput { file_id: 0, text }).expect("lexes");
        let output = parse(&tokens);
        assert!(
            output.errors.is_empty(),
            "unexpected parse errors: {:?}",
            output.errors
        );
        let program = output.program.expect("parse produces a program");
        lower(&program, vec![], vec![]).expect("lowers without errors")
    }

    fn rule_conditions_and_actions(hir: &HirProgram) -> (&Vec<HirExpr>, &Vec<HirStmt>) {
        let HirRuleEntry::Rule(rule) = &hir.rules[0] else {
            panic!("expected a rule");
        };
        (&rule.conditions, &rule.actions)
    }

    #[test]
    fn producer_emits_the_v2_ordered_switch_contract() {
        let hir = lower_ok(
            "globalvar value\nrule \"r\":\n    @Event global\n    switch value:\n        default:\n            value = 1\n        case 2:\n            value = 2\n",
        );
        assert_eq!(hir.protocol.name, "wright/opy-hir");
        assert_eq!(hir.protocol.version, "2.0.0");
        let value = serde_json::to_value(&hir).expect("HIR must serialize");
        let switch = &value["rules"][0]["actions"][0];
        assert!(switch.get("arms").is_some());
        assert!(switch.get("cases").is_none());
        assert!(switch.get("default").is_none());
    }

    #[test]
    fn receiver_calls_lower_to_receiver_call_hir() {
        let hir = lower_ok(
            "globalvar target\nrule \"r\":\n    @Event eachPlayer\n    eventPlayer.setMoveSpeed(100)\n    target.setMoveSpeed(50)\n",
        );
        let (_, actions) = rule_conditions_and_actions(&hir);
        assert_eq!(actions.len(), 2);

        let HirStmt::Expr { expr, .. } = &actions[0] else {
            panic!("expected expression statement");
        };
        let HirExpr::ReceiverCall {
            receiver,
            name,
            args,
            ..
        } = expr.as_ref()
        else {
            panic!("expected receiver call, got {expr:?}");
        };
        assert_eq!(name, "setMoveSpeed");
        assert!(matches!(receiver.as_ref(), HirExpr::EventPlayer { .. }));
        assert_eq!(args.len(), 1);
        assert!(matches!(&args[0], HirExpr::Number { .. }));

        let HirStmt::Expr { expr, .. } = &actions[1] else {
            panic!("expected expression statement");
        };
        let HirExpr::ReceiverCall { receiver, name, .. } = expr.as_ref() else {
            panic!("expected receiver call, got {expr:?}");
        };
        assert_eq!(name, "setMoveSpeed");
        assert!(
            matches!(receiver.as_ref(), HirExpr::GlobalVar { name, .. } if name == "target"),
            "globalvar receiver must resolve to a GlobalVar"
        );
    }

    #[test]
    fn bare_variable_member_expression_preserves_receiver_and_member() {
        let hir = lower_ok(
            "globalvar A\nplayervar B\nrule \"receiver\":\n    @Event eachPlayer\n    A = B.C\n",
        );
        let HirStmt::Assign { value, .. } = &hir
            .rules
            .iter()
            .find_map(|entry| {
                let RuleEntry::Rule(rule) = entry else {
                    return None;
                };
                rule.actions.first()
            })
            .expect("assignment")
        else {
            panic!("expected assignment");
        };
        let HirExpr::Member {
            receiver, member, ..
        } = value.as_ref()
        else {
            panic!("expected opaque member expression, got {value:?}");
        };
        assert_eq!(member, "C");
        assert!(matches!(receiver.as_ref(), HirExpr::GlobalVar { name, .. } if name == "B"));
    }

    #[test]
    fn context_player_components_lower_to_member_expressions() {
        // `x`/`y`/`z` are reserved member names in the pinned reference: on a
        // context-player receiver they resolve unconditionally to the vector
        // component path instead of a player-variable lookup (#414).
        for (receiver_name, event) in [
            ("eventPlayer", "eachPlayer"),
            ("hostPlayer", "eachPlayer"),
            ("localPlayer", "eachPlayer"),
            ("attacker", "playerDied"),
            ("victim", "playerDied"),
        ] {
            for axis in ["x", "y", "z"] {
                let hir = lower_ok(&format!(
                    "globalvar g\nrule \"r\":\n    @Event {event}\n    g = {receiver_name}.{axis}\n"
                ));
                let HirRuleEntry::Rule(rule) = &hir.rules[0] else {
                    panic!("expected a rule");
                };
                let HirStmt::Assign { value, .. } = &rule.actions[0] else {
                    panic!("expected an assign statement");
                };
                let HirExpr::Member {
                    receiver, member, ..
                } = value.as_ref()
                else {
                    panic!("{receiver_name}.{axis}: expected member expression, got {value:?}");
                };
                assert_eq!(member, axis, "{receiver_name}.{axis}");
                match receiver_name {
                    "eventPlayer" => {
                        assert!(matches!(receiver.as_ref(), HirExpr::EventPlayer { .. }))
                    }
                    "hostPlayer" => {
                        assert!(matches!(receiver.as_ref(), HirExpr::HostPlayer { .. }))
                    }
                    _ => assert!(
                        matches!(receiver.as_ref(), HirExpr::Call { name, .. } if name == receiver_name),
                        "{receiver_name}.{axis}: receiver must keep its source identity"
                    ),
                }
            }
        }
    }

    #[test]
    fn context_player_non_component_members_stay_player_variables() {
        let value = lowered_value(
            "globalvar g\nplayervar pos\nrule \"r\":\n    @Event eachPlayer\n    g = eventPlayer.pos\n",
        );
        assert!(
            matches!(&value, HirExpr::PlayerVar { name, .. } if name == "pos"),
            "declared playervar member must stay a player-variable reference, got {value:?}"
        );

        let value = lowered_value(
            "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    g = eventPlayer.X\n",
        );
        assert!(
            matches!(&value, HirExpr::PlayerVar { name, .. } if name == "X"),
            "default-var member must stay a player-variable reference, got {value:?}"
        );

        let error = compile_error(&action_source("g = eventPlayer.pos"), 4);
        assert_eq!(error.code, "unknown-member");
    }

    #[test]
    fn a_bare_player_variable_name_is_an_unknown_identifier() {
        // A bare name resolves only to a global variable or function in the
        // pinned reference; a declared `playervar` is reachable only through
        // the `player.name` member form, so the bare name rejects
        // ("Unknown function name 'target'" upstream) with the structured
        // `unknown-identifier` diagnostic (#532).
        for statement in [
            "target = eventPlayer",
            "eventPlayer.setMoveSpeed(target)",
            "for target in range(3):\n        pass",
        ] {
            let error = compile_error(
                &format!("playervar target\nrule \"r\":\n    @Event eachPlayer\n    {statement}\n"),
                4,
            );
            assert_eq!(error.code, "unknown-identifier", "statement: {statement}");
        }
    }

    #[test]
    fn rule_prefix_template_is_global_and_subroutine_identity_is_preserved() {
        let text = "rule \"before\":\n    pass\ndef source_name():\n    @Name \"Friendly\"\n    pass\nrule \"after\":\n    pass\n";
        let tokens = lex(LexInput { file_id: 0, text }).expect("lexes");
        let output = parse(&tokens);
        assert!(
            output.errors.is_empty(),
            "unexpected parse errors: {:?}",
            output.errors
        );
        let program = output.program.expect("program");
        let preprocessing = PreprocessingState {
            rule_prefix_template: Some(crate::hir::types::DirectiveValue {
                value: "f\"[{$pathTitle.replace('_', ' ')}] {$rule}\" if $rule and not $isDelimiter else $rule".to_string(),
                span: None,
            }),
            ..PreprocessingState::default()
        };
        let hir = lower_with_preprocessing(
            &program,
            vec![SourceFile {
                id: 0,
                path: "main.opy".to_string(),
            }],
            vec![],
            &preprocessing,
        )
        .expect("lowers");
        let names: Vec<_> = hir
            .rules
            .iter()
            .map(|entry| match entry {
                HirRuleEntry::Rule(rule) => rule.name.clone(),
                HirRuleEntry::SubroutineDef { name, .. } => name.clone(),
            })
            .collect();
        assert_eq!(
            names,
            vec!["[Main] before", "[Main] Friendly", "[Main] after"]
        );
        let HirRuleEntry::SubroutineDef {
            name, source_name, ..
        } = &hir.rules[1]
        else {
            panic!("expected subroutine definition");
        };
        assert_eq!(name, "[Main] Friendly");
        assert_eq!(source_name, "source_name");
    }

    #[test]
    fn receiver_call_values_lower_in_conditions() {
        let hir = lower_ok(
            "rule \"r\":\n    @Event eachPlayer\n    @Condition eventPlayer.isAlive()\n    eventPlayer.teleport(eventPlayer.getPosition())\n",
        );
        let (conditions, actions) = rule_conditions_and_actions(&hir);
        assert_eq!(conditions.len(), 1);
        let HirExpr::ReceiverCall { name, args, .. } = &conditions[0] else {
            panic!("expected receiver call condition, got {:?}", conditions[0]);
        };
        assert_eq!(name, "isAlive");
        assert_eq!(args.len(), 0);

        let HirStmt::Expr { expr, .. } = &actions[0] else {
            panic!("expected expression statement");
        };
        let HirExpr::ReceiverCall {
            name,
            args,
            receiver,
            ..
        } = expr.as_ref()
        else {
            panic!("expected receiver call, got {expr:?}");
        };
        assert_eq!(name, "teleport");
        assert!(matches!(receiver.as_ref(), HirExpr::EventPlayer { .. }));
        assert_eq!(args.len(), 1);
        assert!(matches!(
            &args[0],
            HirExpr::ReceiverCall { name, .. } if name == "getPosition"
        ));
    }

    #[test]
    fn format_string_receiver_stays_a_format_node() {
        let hir = lower_ok(
            "rule \"r\":\n    @Event global\n    print(\"{} points\".format(len([1, 2])))\n",
        );
        let (_, actions) = rule_conditions_and_actions(&hir);
        let HirStmt::Expr { expr, .. } = &actions[0] else {
            panic!("expected expression statement");
        };
        assert!(
            has_format(expr),
            "string `.format()` must lower to a Format node"
        );
    }

    fn has_format(expr: &HirExpr) -> bool {
        match expr {
            HirExpr::Format { .. } => true,
            HirExpr::Call { args, .. } => args.iter().any(has_format),
            HirExpr::ReceiverCall { args, .. } => args.iter().any(has_format),
            _ => false,
        }
    }

    fn lowered_value(source: &str) -> HirExpr {
        let program = crate::compile(source, "test.opy", std::path::Path::new(""))
            .unwrap_or_else(|error| panic!("compile failed: {error}"));
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected a rule");
        };
        let HirStmt::Assign { value, .. } = &rule.actions[0] else {
            panic!("expected an assign statement");
        };
        (**value).clone()
    }

    #[test]
    fn chase_time_reeval_none_lowers_to_the_catalog_enum() {
        let value = lowered_value(
            "globalvar g\nrule \"r\":\n    @Event global\n    g = ChaseTimeReeval.NONE\n",
        );
        assert_enum(&value, "ChaseTimeReeval", "NONE");
    }

    #[test]
    fn chase_time_reeval_destination_and_duration_lowers_to_the_catalog_enum() {
        let value = lowered_value(
            "globalvar g\nrule \"r\":\n    @Event global\n    g = ChaseTimeReeval.DESTINATION_AND_DURATION\n",
        );
        assert_enum(&value, "ChaseTimeReeval", "DESTINATION_AND_DURATION");
    }

    #[test]
    fn chase_rate_reeval_members_lower_to_the_catalog_enum() {
        for member in ["NONE", "DESTINATION_AND_RATE"] {
            let source = format!(
                "globalvar g\nrule \"r\":\n    @Event global\n    g = ChaseRateReeval.{member}\n"
            );
            assert_enum(&lowered_value(&source), "ChaseRateReeval", member);
        }
    }

    fn assert_enum(value: &HirExpr, domain: &str, member: &str) {
        match value {
            HirExpr::Enum {
                value_type, value, ..
            } => {
                assert_eq!(value_type, domain);
                assert_eq!(value, member);
            }
            other => panic!("expected enum {domain}.{member}, got {other:?}"),
        }
    }

    #[test]
    fn unknown_chase_time_reeval_member_is_rejected_by_the_catalog() {
        let error = crate::compile(
            "globalvar g\nrule \"r\":\n    @Event global\n    g = ChaseTimeReeval.NOPE\n",
            "test.opy",
            std::path::Path::new(""),
        )
        .expect_err("unknown catalog member must be rejected");
        assert_eq!(error.code, "unknown-enum-member");
    }

    #[test]
    fn unknown_enum_receiver_is_an_unsupported_member_error() {
        let error = crate::compile(
            "globalvar g\nrule \"r\":\n    @Event global\n    g = NotARealEnum.MEMBER\n",
            "test.opy",
            std::path::Path::new(""),
        )
        .expect_err("an unknown enum type must fail");
        assert_eq!(error.code, "unsupported-member");
        let span = error.span.expect("the error is source-located");
        assert_eq!(span.start.line, 4);
    }

    #[test]
    fn unknown_texture_member_is_rejected_by_the_source_surface() {
        let error = crate::compile(
            "globalvar g\nrule \"r\":\n    @Event global\n    g = Texture.NOT_A_TEXTURE\n",
            "test.opy",
            std::path::Path::new(""),
        )
        .expect_err("unknown texture member must be rejected");
        assert_eq!(error.code, "unknown-texture-member");
    }

    fn compile_error(source: &str, line: u32) -> OpyError {
        let error = crate::compile(source, "test.opy", std::path::Path::new(""))
            .expect_err("expected a compile failure");
        let span = error.span.expect("the error is source-located");
        assert_eq!(span.start.line, line, "code '{}'", error.code);
        error
    }

    fn action_source(statement: &str) -> String {
        format!("globalvar g\nrule \"r\":\n    @Event global\n    {statement}\n")
    }

    #[test]
    fn unknown_action_names_the_nearest_spellings_in_message() {
        let error = compile_error(&action_source("createHudText(g, \"t\")"), 4);
        assert_eq!(error.code, "unknown-action");
        assert!(
            error.message.contains("(did you mean 'hudText'"),
            "message: {}",
            error.message
        );
    }

    #[test]
    fn unknown_action_finds_member_action_names() {
        let error = compile_error(&action_source("startForcingPlayerToBeHero(g)"), 4);
        assert_eq!(error.code, "unknown-action");
        assert!(
            error.message.contains("(did you mean 'startForcingHero'"),
            "message: {}",
            error.message
        );
    }

    #[test]
    fn unknown_member_names_the_nearest_member_spellings() {
        let error = compile_error(&action_source("eventPlayer.teleprt(g)"), 4);
        assert_eq!(error.code, "unknown-member");
        assert_eq!(
            error.message,
            "unknown member 'teleprt' (did you mean 'teleport'?)"
        );
    }

    #[test]
    fn unknown_value_includes_the_by_name_special_calls() {
        let error = compile_error(&action_source("g = sroted([1])"), 4);
        assert_eq!(error.code, "unknown-value");
        assert!(
            error.message.contains("'sorted'"),
            "message: {}",
            error.message
        );
    }

    #[test]
    fn unknown_enum_member_names_the_domain_surface() {
        let error = compile_error(&action_source("g = Hero.SOLDIER76"), 4);
        assert_eq!(error.code, "unknown-enum-member");
        assert!(
            error.message.contains("'SOLDIER'"),
            "message: {}",
            error.message
        );
    }

    #[test]
    fn hud_reeval_string_and_color_alias_resolves_and_ranks() {
        let hir = crate::compile(
            &action_source("g = HudReeval.VISIBILITY_STRING_AND_COLOR"),
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("the accepted alternate resolves");
        let RuleEntry::Rule(rule) = &hir.rules[0] else {
            panic!("expected a rule");
        };
        let HirStmt::Assign { value, .. } = &rule.actions[0] else {
            panic!("expected an assignment");
        };
        assert!(matches!(
            value.as_ref(),
            HirExpr::Enum { value_type, value, .. }
                if value_type == "HudReeval" && value == "VISIBLE_TO_STRING_AND_COLOR"
        ));

        let error = compile_error(
            &action_source("g = HudReeval.VISIBILITY_STRING_AND_COLO"),
            4,
        );
        assert_eq!(error.code, "unknown-enum-member");
        assert!(
            error.message.starts_with(
                "enum 'HudReeval' has no member 'VISIBILITY_STRING_AND_COLO' \
                              (did you mean 'VISIBILITY_STRING_AND_COLOR'"
            ),
            "message: {}",
            error.message
        );
    }

    #[test]
    fn unknown_custom_enum_member_names_the_declared_members() {
        let error = compile_error(
            "globalvar g\nenum Dir:\n    NORTH\n    SOUTH\nrule \"r\":\n    @Event global\n    g = Dir.NROTH\n",
            7,
        );
        assert_eq!(error.code, "unknown-enum-member");
        assert_eq!(
            error.message,
            "enum 'Dir' has no member 'NROTH' (did you mean 'NORTH'?)"
        );
    }

    #[test]
    fn chase_over_time_resolves_and_compiles_with_reference_signatures() {
        let hir = crate::compile(
            &action_source("chaseOverTime(g, 10, 3, ChaseTimeReeval.NONE)"),
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("reference-supported chaseOverTime compiles");
        let RuleEntry::Rule(rule) = &hir.rules[0] else {
            panic!("expected a rule");
        };
        let HirStmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected expression statement");
        };
        let HirExpr::Call { name, args, .. } = expr.as_ref() else {
            panic!("expected a call, got {expr:?}");
        };
        assert_eq!(name, "chaseOverTime");
        assert_eq!(args.len(), 4);
        assert!(matches!(
            &args[3],
            HirExpr::Enum { value_type, value, .. }
                if value_type == "ChaseTimeReeval" && value == "NONE"
        ));

        let hir = crate::compile(
            &action_source("chaseOverTime(g, 10, 3)"),
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("default-reevaluation chaseOverTime compiles");
        let RuleEntry::Rule(rule) = &hir.rules[0] else {
            panic!("expected a rule");
        };
        let HirStmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected expression statement");
        };
        let HirExpr::Call { args, .. } = expr.as_ref() else {
            panic!("expected a call");
        };
        assert_eq!(args.len(), 4);
        assert!(matches!(
            &args[3],
            HirExpr::Enum { value_type, value, .. }
                if value_type == "ChaseTimeReeval" && value == "DESTINATION_AND_DURATION"
        ));
    }

    #[test]
    fn is_game_in_progress_resolves_as_a_builtin_value() {
        let hir = crate::compile(
            &action_source("@Condition isGameInProgress() == true"),
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("reference-supported isGameInProgress compiles");
        let RuleEntry::Rule(rule) = &hir.rules[0] else {
            panic!("expected a rule");
        };
        assert!(matches!(&rule.conditions[0], HirExpr::Binary { .. }));
    }

    #[test]
    fn enum_gated_members_resolve_through_the_manifest() {
        let source = "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    \
            @Condition eventPlayer.getThrottle() != vect(0, 0, 0)\n    \
            @Condition worldVector(vect(1, 2, 3), eventPlayer, Transform.ROTATION) != vect(0, 0, 0)\n    \
            eventPlayer.setInvisibility(Invis.ALL)\n    \
            eventPlayer.setStatusEffect(eventPlayer, Status.ROOTED, 2)\n";
        let hir = crate::compile(source, "test.opy", std::path::Path::new(""))
            .expect("enum-gated members compile");
        let RuleEntry::Rule(rule) = &hir.rules[0] else {
            panic!("expected a rule");
        };
        assert_eq!(rule.actions.len(), 2);
    }

    #[test]
    fn get_players_in_radius_fills_reference_enum_defaults() {
        // 2-argument form fills Team.ALL and LosCheck.OFF (reference
        // emission: `Players Within Radius(..., All Teams, Off)`).
        let hir = crate::compile(
            "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    \
             @Condition len(getPlayersInRadius(eventPlayer.getPosition(), 10)) > 0\n    \
             disableInspector()\n",
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("getPlayersInRadius with defaults compiles");
        let RuleEntry::Rule(rule) = &hir.rules[0] else {
            panic!("expected a rule");
        };
        let HirExpr::Binary { left, .. } = &rule.conditions[0] else {
            panic!("expected a comparison");
        };
        let HirExpr::Call { name, args, .. } = left.as_ref() else {
            panic!("expected len call");
        };
        assert_eq!(name, "len");
        let HirExpr::Call { name, args, .. } = &args[0] else {
            panic!("expected getPlayersInRadius call");
        };
        assert_eq!(name, "getPlayersInRadius");
        assert_eq!(args.len(), 4);
        assert!(matches!(
            &args[2],
            HirExpr::Enum { value_type, value, .. }
                if value_type == "Team" && value == "ALL"
        ));
        assert!(matches!(
            &args[3],
            HirExpr::Enum { value_type, value, .. }
                if value_type == "LosCheck" && value == "OFF"
        ));
    }

    #[test]
    fn value_call_in_action_position_is_rejected() {
        let error = compile_error(&action_source("isGameInProgress()"), 4);
        assert_eq!(error.code, "value-in-action-position");
    }

    #[test]
    fn value_member_in_action_position_is_rejected() {
        let error = compile_error(
            "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    eventPlayer.isAlive()\n",
            4,
        );
        assert_eq!(error.code, "value-in-action-position");
    }

    fn first_action_expr(source: &str) -> HirExpr {
        let program = crate::compile(source, "test.opy", std::path::Path::new(""))
            .unwrap_or_else(|error| panic!("compile failed: {error}"));
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected a rule");
        };
        match &rule.actions[0] {
            HirStmt::Expr { expr, .. } => (**expr).clone(),
            HirStmt::Assign { value, .. } => (**value).clone(),
            other => panic!("expected an expression or assignment, got {other:?}"),
        }
    }

    fn strip_spans(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("span");
                map.remove("name_span");
                for nested in map.values_mut() {
                    strip_spans(nested);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    strip_spans(item);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn chase_keyword_forms_dispatch_to_the_concrete_chase_functions() {
        // The reference `chase` form: `rate = …` dispatches to chaseAtRate
        // with the ChaseRateReeval domain, `duration = …` to chaseOverTime
        // with the ChaseTimeReeval domain; the `ChaseReeval` member resolves
        // only through this call context (issue #110).
        let expr = first_action_expr(&action_source("chase(g, 10, rate=2, ChaseReeval.NONE)"));
        let HirExpr::Call { name, args, .. } = &expr else {
            panic!("expected a call, got {expr:?}");
        };
        assert_eq!(name, "chaseAtRate");
        assert!(matches!(
            &args[3],
            HirExpr::Enum { value_type, value, .. }
                if value_type == "ChaseRateReeval" && value == "NONE"
        ));

        let expr = first_action_expr(&action_source(
            "chase(g, 10, duration=3, ChaseReeval.DESTINATION_AND_DURATION)",
        ));
        let HirExpr::Call { name, args, .. } = &expr else {
            panic!("expected a call, got {expr:?}");
        };
        assert_eq!(name, "chaseOverTime");
        assert!(matches!(
            &args[3],
            HirExpr::Enum { value_type, value, .. }
                if value_type == "ChaseTimeReeval" && value == "DESTINATION_AND_DURATION"
        ));

        let expr = first_action_expr(
            "playervar P\nrule \"r\":\n    @Event eachPlayer\n    \
             chase(eventPlayer.P, 0, rate=1, ChaseReeval.NONE)\n",
        );
        let HirExpr::Call { name, args, .. } = &expr else {
            panic!("expected a call, got {expr:?}");
        };
        assert_eq!(name, "chaseAtRate");
        assert!(matches!(&args[0], HirExpr::PlayerVar { .. }));
    }

    #[test]
    fn chase_reeval_is_only_a_standalone_identity_inside_the_chase_context() {
        let error = compile_error(&action_source("g = ChaseReeval.NONE"), 4);
        assert_eq!(error.code, "unsupported-member");

        let expr = first_action_expr(&action_source(
            "chase(g, 10, rate=2, ChaseReeval.DESTINATION_AND_DURATION)",
        ));
        let HirExpr::Call { name, args, .. } = &expr else {
            panic!("expected a call, got {expr:?}");
        };
        assert_eq!(name, "chaseAtRate");
        assert!(matches!(
            &args[3],
            HirExpr::Enum { value_type, value, .. }
                if value_type == "ChaseRateReeval" && value == "DESTINATION_AND_DURATION"
        ));

        let expr = first_action_expr(&action_source("chase(g, 10, rate=2, 5)"));
        let HirExpr::Call { name, args, .. } = &expr else {
            panic!("expected a call, got {expr:?}");
        };
        assert_eq!(name, "chase");
        assert!(matches!(&args[3], HirExpr::Number { .. }));
    }

    #[test]
    fn chase_requires_the_keyword_rate_or_duration_third_argument() {
        let error = compile_error(&action_source("chase(g, 10, 2, ChaseReeval.NONE)"), 4);
        assert_eq!(error.code, "keyword-required");
        assert!(error.message.contains("rate"));
    }

    #[test]
    fn chase_family_requires_a_variable_first_argument() {
        // The reference rejects non-variable first arguments for the chase
        // family ("Expected variable for 1st argument of function
        // 'chaseOverTime'", issue #110) — the variable kind also selects
        // the global/player emission form.
        let error = compile_error(&action_source("chase(10, 10, rate=2, ChaseReeval.NONE)"), 4);
        assert_eq!(error.code, "invalid-argument");

        let error = compile_error(
            &action_source("chaseOverTime(10, 0, 30, ChaseTimeReeval.NONE)"),
            4,
        );
        assert_eq!(error.code, "invalid-argument");

        let error = compile_error(
            &action_source("chaseAtRate(10, 0, 2, ChaseRateReeval.NONE)"),
            4,
        );
        assert_eq!(error.code, "invalid-argument");

        crate::compile(
            &action_source("chaseAtRate(g, 0, 2, ChaseRateReeval.NONE)"),
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("a variable first argument compiles");
    }

    #[test]
    fn keyword_binding_matches_positional_binding_in_hir() {
        fn without_spans(expr: &HirExpr) -> serde_json::Value {
            let mut value = serde_json::to_value(expr).unwrap();
            strip_spans(&mut value);
            value
        }
        let keyword = without_spans(&first_action_expr(&action_source(
            "chaseOverTime(g, 10, duration=3)",
        )));
        let positional = without_spans(&first_action_expr(&action_source(
            "chaseOverTime(g, 10, 3)",
        )));
        assert_eq!(keyword, positional);

        let keyword = without_spans(&first_action_expr(&action_source("wait(time=1)")));
        let positional = without_spans(&first_action_expr(&action_source("wait(1)")));
        assert_eq!(keyword, positional);

        let keyword = without_spans(&first_action_expr(&action_source(
            "wait(waitBehavior=Wait.IGNORE_CONDITION, time=2)",
        )));
        let positional = without_spans(&first_action_expr(&action_source("wait(2)")));
        assert_eq!(keyword, positional);

        let keyword = without_spans(&first_action_expr(&action_source(
            "g = vect(x=1, y=2, z=3)",
        )));
        let positional = without_spans(&first_action_expr(&action_source("g = vect(1, 2, 3)")));
        assert_eq!(keyword, positional);
    }

    #[test]
    fn keyword_binding_diagnostics_are_structured_and_source_located() {
        let error = compile_error(&action_source("chaseOverTime(g, 10, bogus=1)"), 4);
        assert_eq!(error.code, "unknown-keyword");
        assert!(error.message.contains("bogus"));

        let error = compile_error(
            &action_source(
                "chaseOverTime(g, 10, 3, ChaseTimeReeval.NONE, \
                 reevaluation=ChaseTimeReeval.NONE)",
            ),
            4,
        );
        assert_eq!(error.code, "duplicate-argument");

        let error = compile_error(&action_source("chaseOverTime(g, duration=3, 5)"), 4);
        assert_eq!(error.code, "positional-after-keyword");

        let error = compile_error(&action_source("chaseOverTime(g, 10)"), 4);
        assert_eq!(error.code, "missing-argument");

        // Positional-only parameter bound by keyword (`chase`'s leading
        // arguments; the reference rejects the keyword form).
        let error = compile_error(
            &action_source("chase(variable=g, destination=10, rate=2, ChaseReeval.NONE)"),
            4,
        );
        assert_eq!(error.code, "unknown-keyword");
    }

    #[test]
    fn keyword_arguments_are_rejected_for_reference_special_cases() {
        // The reference routes `range`, `random.*`, and `.format` around its
        // generic keyword binder; keyword arguments fail deterministically.
        let error = compile_error(
            "globalvar g\nrule \"r\":\n    @Event global\n    \
             for I in range(start=0, stop=3):\n        debug(I)\n",
            4,
        );
        assert_eq!(error.code, "keyword-unsupported");

        let error = compile_error(&action_source("g = random.uniform(min=1, max=2)"), 4);
        assert_eq!(error.code, "keyword-unsupported");

        let error = compile_error(&action_source("print(\"{} points\".format(value=1))"), 4);
        assert_eq!(error.code, "keyword-unsupported");
    }

    #[test]
    fn sorted_rejects_keyword_arguments_beyond_the_key_lambda() {
        // #437: the reference's special `sorted` parse path only strips a
        // `key=` prefix inside the second argument; other keyword spellings
        // and a misplaced `key=` fail upstream parsing.
        let error = compile_error(&action_source("g = sorted([1, 2], bogus=lambda v: v)"), 4);
        assert_eq!(error.code, "unknown-keyword");

        let error = compile_error(
            &action_source("g = sorted(array=[1, 2], key=lambda v: -v)"),
            4,
        );
        assert_eq!(error.code, "unknown-keyword");

        let error = compile_error(&action_source("g = sorted(key=lambda v: v)"), 4);
        assert_eq!(error.code, "unknown-keyword");
    }

    #[test]
    fn call_args_reject_parenthesized_keyword_names() {
        // #443: the reference detects keyword arguments at the token level,
        // so `(name)=expr` never binds as a keyword.
        let error = compile_error(&action_source("g = vect((g)=1, y=2, z=3)"), 4);
        assert_eq!(error.code, "parse-error");

        let error = compile_error(&action_source("g = sorted([1, 2], (key)=lambda v: -v)"), 4);
        assert_eq!(error.code, "parse-error");

        crate::compile(
            &action_source("g = vect(x=1, y=2, z=3) + vect((g), 2, 3)"),
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("bare keyword arguments compile");
    }

    #[test]
    fn call_args_reject_trailing_comma() {
        // #444: the reference keeps the empty segment after a trailing comma
        // and rejects it in call argument lists.
        for statement in [
            "wait(1,)",
            "g = vect(1, 2, 3,)",
            "g = sorted([1, 2],)",
            "g = sorted([1, 2], key=lambda v: -v,)",
        ] {
            let error = compile_error(&action_source(statement), 4);
            assert_eq!(error.code, "parse-error", "statement: {statement}");
        }

        crate::compile(
            &action_source("g = [1, 2,]"),
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("array literal trailing comma compiles");
        crate::compile(
            &action_source("g = {\"a\": 1,}[\"a\"]"),
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("dict literal trailing comma compiles");
        crate::compile(
            "rule \"r\":\n    @Event playerDied(victim,)\n    wait(1)\n",
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("@Event argument trailing comma compiles");
    }

    #[test]
    fn binder_lambda_rejects_parenthesized() {
        // #445: the reference detects binder lambdas at the token level, so
        // `(lambda x: e)` never satisfies a lambda-argument position.
        for statement in [
            "g = sorted([1, 2], (lambda v: -v))",
            "g = sorted([1, 2], key=(lambda v: -v))",
            "g = [1, 2].map((lambda v: v))",
            "g = [1, 2].filter((lambda v: v))",
            "g = [1, 2].all((lambda v: v))",
            "g = [1, 2].any((lambda v: v))",
            "g = sorted([1, 2], ((lambda v: -v)))",
        ] {
            let error = compile_error(&action_source(statement), 4);
            assert_eq!(error.code, "lambda-context", "statement: {statement}");
        }

        crate::compile(
            &action_source("g = sorted([1, 2], key=lambda v: -v)"),
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("bare lambda key compiles");
    }

    #[test]
    fn wait_uses_the_reference_keyword_names() {
        // The manifest's `wait` parameter names match the pinned reference
        // (`time`, `waitBehavior`), so `wait(duration=1)` is an unknown
        // keyword exactly like the oracle.
        let error = compile_error(&action_source("wait(duration=1)"), 4);
        assert_eq!(error.code, "unknown-keyword");
        assert!(error.message.contains("duration"));
    }

    #[test]
    fn action_call_in_value_position_is_rejected() {
        let error = compile_error(&action_source("g = wait(1)"), 4);
        assert_eq!(error.code, "action-in-value-position");
    }

    #[test]
    fn missing_required_argument_is_a_source_located_diagnostic() {
        // Too-few calls reject with the reference's missing-argument
        // diagnostic (`chaseOverTime(g, 10)` → "Missing argument 'duration'",
        // issue #110); positional overflow keeps `invalid-arity`.
        let error = compile_error(&action_source("chaseOverTime(g, 10)"), 4);
        assert_eq!(error.code, "missing-argument");
        assert!(error.message.contains("duration"));

        let error = compile_error(&action_source("chaseOverTime(g, 10, 3, 4, 5)"), 4);
        assert_eq!(error.code, "invalid-arity");
    }

    #[test]
    fn missing_member_argument_is_a_source_located_diagnostic() {
        // #106 evidence: `getPlayersInRadius(...).setStatusEffect(eventPlayer,
        // 30)` must reject like the oracle (the `status` argument is
        // missing; the reference: "Missing argument 'status' for function
        // '.setStatusEffect'", issue #110).
        let error = compile_error(
            "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    \
             getPlayersInRadius(eventPlayer.getPosition(), 10).setStatusEffect(eventPlayer, 30)\n",
            4,
        );
        assert_eq!(error.code, "missing-argument");
        assert!(error.message.contains("duration"));
    }

    #[test]
    fn invalid_receiver_categories_are_rejected() {
        let error = compile_error(&action_source("3.append(1)"), 4);
        assert_eq!(error.code, "invalid-receiver");
        assert!(error.message.contains("append"));

        let error = compile_error(&action_source("attacker.append(1)"), 4);
        assert_eq!(error.code, "invalid-receiver");
        assert!(error.message.contains("append"));

        let error = compile_error(&action_source("3.remove(1)"), 4);
        assert_eq!(error.code, "invalid-receiver");
        assert!(error.message.contains("remove"));

        let error = compile_error(&action_source("print(3.format(\"{}\"))"), 4);
        assert_eq!(error.code, "invalid-receiver");
        assert!(error.message.contains("format"));

        crate::compile(
            &action_source("g.append(1)\n    g.remove(1)"),
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("assignable receivers compile");
        crate::compile(
            &action_source("g = \"{}\".format(1)"),
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("a string-literal receiver compiles");
    }

    fn lower_with_manifest(source: &str, manifest: &'static Manifest) -> OpyResult<HirProgram> {
        let tokens = lex(LexInput {
            file_id: 0,
            text: source,
        })
        .expect("lexes");
        let output = parse(&tokens);
        assert!(
            output.errors.is_empty(),
            "unexpected parse errors: {:?}",
            output.errors
        );
        let program = output.program.expect("parse produces a program");
        lower_program(
            &program,
            vec![],
            vec![],
            &PreprocessingState::default(),
            manifest,
            Catalog::builtin_ref().expect("the builtin catalog loads"),
        )
    }

    #[test]
    fn special_enforcement_is_typed_policy_not_manifest_metadata() {
        let mut file: serde_json::Value =
            serde_json::from_str(crate::manifest::MANIFEST_DATA).unwrap();
        for function in file["functions"].as_array_mut().unwrap() {
            match function["id"].as_str().unwrap() {
                "chase" | "chaseAtRate" | "chaseOverTime" => {
                    for param in function["params"].as_array_mut().unwrap() {
                        param.as_object_mut().unwrap().remove("variable");
                    }
                }
                "append" | "remove" | "format" => {
                    function["receiver"] = "Any".into();
                }
                "wait" => function["params"][0]["variable"] = true.into(),
                "getPosition" => function["receiver"] = "Variable".into(),
                _ => {}
            }
        }
        let manifest: &'static Manifest = Box::leak(Box::new(
            Manifest::load(
                &serde_json::to_string(&file).unwrap(),
                crate::manifest::PROBES_DATA,
            )
            .expect("mutated metadata still validates"),
        ));

        for call in [
            "chase(10, 10, rate=2, ChaseReeval.NONE)",
            "chaseAtRate(10, 0, 2, ChaseRateReeval.NONE)",
            "chaseOverTime(10, 0, 30, ChaseTimeReeval.NONE)",
        ] {
            let error = lower_with_manifest(&action_source(call), manifest)
                .expect_err("the chase family still requires a variable");
            assert_eq!(error.code, "invalid-argument", "{call}");
        }
        lower_with_manifest(&action_source("chaseOverTime(g, 10, 3)"), manifest)
            .expect("a variable first argument compiles");

        for call in ["3.append(1)", "3.remove(1)", "print(3.format(\"{}\"))"] {
            let error = lower_with_manifest(&action_source(call), manifest)
                .expect_err("the enforced receiver requirement still rejects");
            assert_eq!(error.code, "invalid-receiver", "{call}");
        }
        lower_with_manifest(&action_source("g.append(1)"), manifest)
            .expect("an assignable receiver compiles");
        lower_with_manifest(&action_source("g = \"{}\".format(1)"), manifest)
            .expect("a string-literal receiver compiles");

        lower_with_manifest(&action_source("wait(1)"), manifest)
            .expect("`wait`'s first argument is not variable-required");
        lower_with_manifest(
            "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    \
             g = eventPlayer.getPosition()\n",
            manifest,
        )
        .expect("`getPosition`'s receiver is not assignable-enforced");
    }

    #[test]
    fn cross_domain_enum_arguments_resolve_as_opaque_identities() {
        let expr = first_action_expr(&action_source("chaseOverTime(g, 10, 3, Invis.ALL)"));
        let HirExpr::Call { args, .. } = &expr else {
            panic!("expected a call, got {expr:?}");
        };
        assert!(matches!(
            &args[3],
            HirExpr::Enum { value_type, value, .. }
                if value_type == "Invis" && value == "ALL"
        ));

        let expr = first_action_expr(&action_source(
            "eventPlayer.setInvisibility(ChaseTimeReeval.NONE)",
        ));
        let HirExpr::ReceiverCall { args, .. } = &expr else {
            panic!("expected a receiver call, got {expr:?}");
        };
        assert!(matches!(
            &args[0],
            HirExpr::Enum { value_type, value, .. }
                if value_type == "ChaseTimeReeval" && value == "NONE"
        ));
    }

    #[test]
    fn non_enum_arguments_for_enum_parameters_are_carried_structurally() {
        let expr = first_action_expr(&action_source("eventPlayer.setInvisibility(g)"));
        let HirExpr::ReceiverCall { args, .. } = &expr else {
            panic!("expected a receiver call, got {expr:?}");
        };
        assert!(matches!(
            &args[0],
            HirExpr::GlobalVar { name, .. } if name == "g"
        ));

        let expr = first_action_expr(&action_source("eventPlayer.setInvisibility(3)"));
        let HirExpr::ReceiverCall { args, .. } = &expr else {
            panic!("expected a receiver call, got {expr:?}");
        };
        assert!(matches!(&args[0], HirExpr::Number { .. }));
    }

    #[test]
    fn unknown_builtins_fail_at_resolution_not_emission() {
        let error = compile_error(&action_source("frobnicate()"), 4);
        assert_eq!(error.code, "unknown-action");

        let error = compile_error(&action_source("g = frobnicate()"), 4);
        assert_eq!(error.code, "unknown-value");

        let error = compile_error(
            "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    eventPlayer.frobnicate()\n",
            4,
        );
        assert_eq!(error.code, "unknown-member");
    }

    #[test]
    fn wright_only_catalog_names_are_rejected() {
        // `createHudText` and `squareRoot` are Workshop emission spellings,
        // not OPY source functions; the pinned reference rejects them, so
        // the manifest does not preserve the accidental acceptance.
        let error = compile_error(&action_source("createHudText(1)"), 4);
        assert_eq!(error.code, "unknown-action");

        let error = compile_error(&action_source("g = squareRoot(9)"), 4);
        assert_eq!(error.code, "unknown-value");
    }

    #[test]
    fn canonical_ids_without_upstream_spellings_are_rejected() {
        // These canonical Workshop ids are not upstream OPY spellings (#410):
        // the oracle rejects them, so they are source diagnostics. Their
        // upstream spellings keep the same canonical identity via `catalogId`.
        for (statement, code) in [
            ("g = evaluateOnce(1)", "unknown-value"),
            ("g = lastCreatedEntity()", "unknown-value"),
            ("g = lastTextId()", "unknown-value"),
            ("g = allTankHeroes()", "unknown-value"),
            ("g = allDamageHeroes()", "unknown-value"),
            ("g = allSupportHeroes()", "unknown-value"),
            ("destroyAllHudText()", "unknown-action"),
        ] {
            let error = compile_error(&action_source(statement), 4);
            assert_eq!(error.code, code, "{statement}");
        }

        let member = "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    eventPlayer.isButtonHeld(Button.PRIMARY_FIRE)\n";
        let error = compile_error(member, 4);
        assert_eq!(error.code, "unknown-member");

        for axis in ["x", "y", "z"] {
            let source = format!(
                "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    g = eventPlayer.{axis}()\n"
            );
            let error = compile_error(&source, 4);
            assert_eq!(error.code, "unknown-member", "{axis}()");
        }

        crate::compile(
            "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    g = evalOnce(1)\n    g = getLastCreatedEntity()\n    g = getLastCreatedText()\n    g = getTankHeroes()\n    g = getDamageHeroes()\n    g = getSupportHeroes()\n    g = eventPlayer.isHoldingButton(Button.PRIMARY_FIRE)\n    destroyAllHudTexts()\n",
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("the upstream spellings compile");
    }

    #[test]
    fn bare_spelled_values_accept_an_empty_call_form() {
        // The reference produces the same AST for a `funcKw`
        // `args === null` name whether written bare or as `name()` (#529):
        // the context players and the `true`/`false`/`null` literals.
        for (name, expect) in [
            ("attacker", "call"),
            ("victim", "call"),
            ("localPlayer", "call"),
            ("eventPlayer", "event-player"),
            ("hostPlayer", "host-player"),
            ("true", "bool"),
            ("false", "bool"),
            ("null", "null"),
        ] {
            let value = lowered_value(&action_source(&format!("g = {name}()")));
            match expect {
                "call" => assert!(
                    matches!(&value, HirExpr::Call { name: n, args, .. } if n == name && args.is_empty()),
                    "{name}() must lower like the bare name, got {value:?}"
                ),
                "event-player" => assert!(
                    matches!(&value, HirExpr::EventPlayer { .. }),
                    "eventPlayer() must lower like the bare name, got {value:?}"
                ),
                "host-player" => assert!(
                    matches!(&value, HirExpr::HostPlayer { .. }),
                    "hostPlayer() must lower like the bare name, got {value:?}"
                ),
                "bool" => assert!(
                    matches!(&value, HirExpr::Bool { value: v, .. } if *v == (name == "true")),
                    "{name}() must lower to the literal, got {value:?}"
                ),
                _ => assert!(
                    matches!(&value, HirExpr::Null { .. }),
                    "null() must lower to the literal, got {value:?}"
                ),
            }
        }

        for statement in ["g = attacker(1)", "g = null(1)", "g = eventPlayer(1)"] {
            let error = compile_error(&action_source(statement), 4);
            assert_eq!(error.code, "invalid-arity", "{statement}");
        }

        for statement in ["attacker()", "eventPlayer()", "true()"] {
            let error = compile_error(&action_source(statement), 4);
            assert_eq!(error.code, "value-in-action-position", "{statement}");
        }

        // `RULE_CONDITION` is a constant spelling, not a `funcKw` name:
        // the reference rejects `RULE_CONDITION()` and so does the call
        // form here.
        let error = compile_error(&action_source("g = RULE_CONDITION()"), 4);
        assert_eq!(error.code, "unknown-value");
    }

    #[test]
    fn generic_member_only_actions_are_rejected() {
        // `setMoveSpeed(eventPlayer, 100)` is not an OPY function: the
        // member form is the reference surface.
        let error = compile_error(&action_source("setMoveSpeed(eventPlayer, 100)"), 4);
        assert_eq!(error.code, "unknown-action");
    }

    #[test]
    fn range_is_for_iterables_only() {
        // Standalone `range(...)` is rejected by the reference; the
        // for-header form keeps 1-3 arguments.
        let error = compile_error(&action_source("@Condition len(range(1, 5, 1)) > 0"), 4);
        assert_eq!(error.code, "invalid-call-context");

        let error = compile_error(&action_source("for g in [1, 2]:\n        debug(g)"), 4);
        assert_eq!(error.code, "invalid-iterable");

        crate::compile(
            &action_source("for g in range(3):\n        debug(g)"),
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("the for-header range form compiles");
    }

    #[test]
    fn source_aliases_resolve_to_canonical_names() {
        let hir = crate::compile(
            &action_source("stopChasingVariable(g)"),
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("the alias target compiles");
        let RuleEntry::Rule(rule) = &hir.rules[0] else {
            panic!("expected a rule");
        };
        let HirStmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected expression statement");
        };
        let HirExpr::Call { name, .. } = expr.as_ref() else {
            panic!("expected a call");
        };
        assert_eq!(name, "stopChasingVariable");

        let hir = crate::compile(
            "globalvar g\nrule \"r\":\n    @Event eachPlayer\n    \
             @Condition eventPlayer.getCurrentHero() != null\n    \
             @Condition eventPlayer.hasStatusEffect(Status.BURNING) == false\n    \
             disableInspector()\n",
            "test.opy",
            std::path::Path::new(""),
        )
        .expect("member aliases compile");
        let RuleEntry::Rule(rule) = &hir.rules[0] else {
            panic!("expected a rule");
        };
        let HirExpr::Binary { left, .. } = &rule.conditions[0] else {
            panic!("expected a comparison");
        };
        let HirExpr::ReceiverCall { name, .. } = left.as_ref() else {
            panic!("expected a receiver call");
        };
        assert_eq!(name, "getHero");
    }

    #[test]
    fn unknown_catalog_enum_members_are_rejected() {
        for source in [
            "globalvar g\nrule \"r\":\n    @Event global\n    g = Color.CYAN\n",
            "globalvar g\nrule \"r\":\n    @Event global\n    g = DynamicEffect.SPARKLES\n",
        ] {
            let error = crate::compile(source, "test.opy", std::path::Path::new(""))
                .expect_err("unknown catalog member must be rejected");
            assert_eq!(error.code, "unknown-enum-member");
        }
    }

    #[test]
    fn default_var_for_binder_resolves_at_all_range_arities() {
        // The agent-lab regression: `for I in range(0, 10):` with `I` not
        // declared. `I` is an OverPy default variable name (A–Z, AA–…), which
        // the pinned reference accepts as an implicit global loop binder
        // (#114). All range arities keep compiling (1, 2, and 3 arguments).
        for (binder, iterable) in [
            ("I", "range(0, 10)"),
            ("I", "range(3)"),
            ("I", "range(1, 5, 2)"),
        ] {
            let hir = lower_ok(&format!(
                "globalvar total\nrule \"r\":\n    @Event global\n    for {binder} in {iterable}:\n        total += {binder}\n"
            ));
            let (_, actions) = rule_conditions_and_actions(&hir);
            let HirStmt::For { variable, body, .. } = &actions[0] else {
                panic!("expected a for statement");
            };
            assert!(
                matches!(variable.as_ref(), HirExpr::GlobalVar { name, .. } if name == "I"),
                "the binder resolves to the implicit global 'I', got {variable:?}"
            );
            assert!(!body.is_empty(), "the loop body lowers");
            let HirStmt::Assign { value, .. } = &body[0] else {
                panic!("expected an assignment in the body");
            };
            let HirExpr::Binary { right, .. } = value.as_ref() else {
                panic!("expected a binary expression");
            };
            assert!(
                matches!(right.as_ref(), HirExpr::GlobalVar { name, .. } if name == "I"),
                "the binder use inside the body resolves to the implicit global"
            );
        }
    }

    #[test]
    fn player_variable_range_binder_preserves_host_player_receiver() {
        let hir = lower_ok(
            "playervar I\nrule \"r\":\n    @Event global\n    for hostPlayer.I in range(3):\n        hostPlayer.I = 1\n",
        );
        let (_, actions) = rule_conditions_and_actions(&hir);
        let HirStmt::For { variable, .. } = &actions[0] else {
            panic!("expected a for statement");
        };
        let HirExpr::PlayerVar {
            player,
            name,
            member_span,
            span,
        } = variable.as_ref()
        else {
            panic!("expected a player-variable binder, got {variable:?}");
        };
        assert_eq!(name, "I");
        assert!(matches!(player.as_ref(), HirExpr::HostPlayer { .. }));
        assert_eq!(span.unwrap().start.line, 4);
        assert_eq!(span.unwrap().start.col, 9);
        assert_eq!(span.unwrap().end.line, 4);
        assert_eq!(span.unwrap().end.col, 21);
        let member_span = member_span.expect("player binder member span");
        assert_eq!(member_span.start.line, 4);
        assert_eq!(member_span.start.col, 20);
        assert_eq!(member_span.end.line, 4);
        assert_eq!(member_span.end.col, 21);
    }

    #[test]
    fn default_var_names_resolve_as_implicit_globals() {
        // Default variable names resolve anywhere a variable may appear,
        // matching the pinned reference (no `globalvar` declaration needed).
        let hir = lower_ok("rule \"r\":\n    @Event global\n    I = 5\n    debug(I)\n");
        let (_, actions) = rule_conditions_and_actions(&hir);
        let HirStmt::Assign { target, .. } = &actions[0] else {
            panic!("expected an assignment");
        };
        assert!(
            matches!(target.as_ref(), HirExpr::GlobalVar { name, .. } if name == "I"),
            "the implicit global resolves, got {target:?}"
        );
        assert_eq!(default_var_index("I"), Some(8));
        assert_eq!(default_var_index("AA"), Some(26));
        assert_eq!(default_var_index("Z"), Some(25));
        assert_eq!(default_var_index("DX"), Some(127));
        assert_eq!(default_var_index("DY"), None);
        assert_eq!(default_var_index("i"), None);
    }

    #[test]
    fn nested_same_name_for_binders_reuse_the_implicit_global() {
        // Nested loops with the same default-var binder reuse the single
        // implicit variable, matching the pinned reference (the inner loop
        // overwrites the same Workshop global — no separate binding).
        let hir = lower_ok(
            "rule \"r\":\n    @Event global\n    for I in range(3):\n        for I in range(2):\n            debug(I)\n",
        );
        let (_, actions) = rule_conditions_and_actions(&hir);
        let HirStmt::For {
            variable: outer,
            body,
            ..
        } = &actions[0]
        else {
            panic!("expected an outer for statement");
        };
        let HirStmt::For {
            variable: inner, ..
        } = &body[0]
        else {
            panic!("expected an inner for statement");
        };
        assert!(
            matches!(outer.as_ref(), HirExpr::GlobalVar { name, .. } if name == "I")
                && matches!(inner.as_ref(), HirExpr::GlobalVar { name, .. } if name == "I"),
            "both loops bind the same implicit global (spans differ per binder site)"
        );
    }

    #[test]
    fn undeclared_lowercase_binder_is_still_an_unknown_identifier() {
        // A lowercase undeclared binder is not a default variable name; the
        // pinned reference rejects the program ("Unknown function name"), and
        // Wright reports the same reject with the structured
        // `unknown-identifier` diagnostic (#114).
        let error = compile_error(
            "rule \"r\":\n    @Event global\n    for i in range(3):\n        debug(i)\n",
            3,
        );
        assert_eq!(error.code, "unknown-identifier");
        let span = error.span.expect("the error is source-located");
        assert_eq!(span.start.line, 3);
    }

    #[test]
    fn syntax_constructs_lower_to_provenance_preserving_hir() {
        let hir = lower_ok(
            "globalvar x\nrule \"r\":\n    @Event global\n    do:\n        x = {\"x\": 1}[\"x\"]\n    while x not in [2, 3]\n    switch x:\n        case 0x10:\n            x = 1 in [1, 2]\n        default:\n            x = 2\n    x = [value * 2 for value, index in [1, 2] if value > index]\n    x = sorted([1, 2], key=lambda value: value)\n    x = w\"wide\"\n",
        );
        let (_, actions) = rule_conditions_and_actions(&hir);
        let HirStmt::DoWhile { condition, .. } = &actions[0] else {
            panic!("expected do-while");
        };
        assert!(matches!(condition.as_ref(), HirExpr::Binary { op, .. } if op == "not in"));
        let HirStmt::Switch { arms, .. } = &actions[1] else {
            panic!("expected switch");
        };
        assert_eq!(arms.len(), 2);
        let HirSwitchArm::Case {
            value: case_value,
            body,
            ..
        } = &arms[0]
        else {
            panic!("expected case arm");
        };
        assert!(
            matches!(case_value.as_ref(), HirExpr::Number { value, .. } if *value == 0x10 as f64)
        );
        let HirStmt::Assign { value, .. } = &body[0] else {
            panic!("expected case assignment");
        };
        assert!(matches!(value.as_ref(), HirExpr::Binary { op, .. } if op == "in"));
        let HirSwitchArm::Default { body, .. } = &arms[1] else {
            panic!("expected default arm");
        };
        assert!(matches!(body[0], HirStmt::Assign { .. }));
        let HirStmt::Assign { value, .. } = &actions[2] else {
            panic!("expected comprehension assignment");
        };
        assert!(matches!(value.as_ref(), HirExpr::Comprehension { .. }));
        let HirStmt::Assign { value, .. } = &actions[3] else {
            panic!("expected sorted assignment");
        };
        assert!(
            matches!(value.as_ref(), HirExpr::Call { name, args, .. } if name == "sorted" && matches!(&args[1], HirExpr::Lambda { body, .. } if matches!(body.as_ref(), HirExpr::Local { name, .. } if name == "value")))
        );
        let HirStmt::Assign { value, .. } = &actions[4] else {
            panic!("expected string assignment");
        };
        assert!(
            matches!(value.as_ref(), HirExpr::StringModifier { modifier, .. } if modifier == "w")
        );
    }

    #[test]
    fn syntax_rejects_reference_invalid_bare_dict_and_lambda() {
        let dict_error = compile_error(
            "globalvar x\nrule \"r\":\n    @Event global\n    x = {\"x\": 1}\n",
            4,
        );
        assert_eq!(dict_error.code, "dict-access");
        let lambda_error = compile_error(
            "globalvar x\nrule \"r\":\n    @Event global\n    x = lambda value: value\n",
            4,
        );
        assert_eq!(lambda_error.code, "lambda-context");
    }

    #[test]
    fn do_while_requires_rule_or_definition_prefix_position() {
        let error = compile_error(
            "globalvar value\nrule \"r\":\n    @Event global\n    value = 1\n    do:\n        value += 1\n    while value < 2\n",
            5,
        );
        assert_eq!(error.code, "do-while-placement");
        assert_eq!(
            error.message,
            "do-while must be at the beginning of a rule, subroutine, or do-while body; only pass statements may precede it"
        );
    }
}
