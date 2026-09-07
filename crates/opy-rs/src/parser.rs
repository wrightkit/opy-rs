//! The indentation-aware `.opy` CST parser.
//!
//! Consumes the expanded token stream from [`crate::preprocess`] and builds a
//! [`cst::Program`]. Parsing is deterministic and corpus-backed; malformed
//! input produces structured [`OpyError`]s rather than panics, and the
//! parser recovers at statement/line boundaries so multiple useful errors are
//! reported. The returned [`ParseOutput`] carries either a complete program
//! or the collected errors (never both).

use crate::cst::{
    Annotation, AnnotationArg, CallArg, Decl, DictEntry, Event, Expr, IfBranch, Program, Rule,
    RuleEntry, Stmt, SwitchArm, TopLevel,
};
use crate::diag::{OpyError, Position, Span};
use crate::lexer::{Token, TokenKind};

/// The outcome of a parse.
#[derive(Debug, Default)]
pub struct ParseOutput {
    /// The parsed program, present only when no errors were collected.
    pub program: Option<Program>,
    /// Every structured error collected during the parse.
    pub errors: Vec<OpyError>,
}

/// Parse an expanded token stream into a CST program.
pub fn parse(tokens: &[Token]) -> ParseOutput {
    parse_with_options(tokens, false)
}

/// Parse with the global redeclaration policy observed by the pinned oracle.
pub fn parse_with_options(tokens: &[Token], allow_macro_redeclaration: bool) -> ParseOutput {
    let mut parser = Parser {
        tokens,
        pos: 0,
        errors: Vec::new(),
        allow_macro_redeclaration,
    };
    let program = parser.parse_program();
    if parser.errors.is_empty() {
        ParseOutput {
            program: Some(program),
            errors: Vec::new(),
        }
    } else {
        ParseOutput {
            program: None,
            errors: parser.errors,
        }
    }
}

mod declarations;
mod definitions;
mod expressions;
mod statements;

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    errors: Vec<OpyError>,
    allow_macro_redeclaration: bool,
}

fn is_identifier(text: &str) -> bool {
    !text.is_empty()
        && text.chars().enumerate().all(|(index, ch)| {
            if index == 0 {
                ch.is_ascii_alphabetic() || ch == '_'
            } else {
                ch.is_ascii_alphanumeric() || ch == '_'
            }
        })
}

fn unquote_annotation_arg(text: &str) -> String {
    text.strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(text)
        .to_string()
}

impl Parser<'_> {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_kind(&self) -> TokenKind {
        self.peek().kind
    }

    fn peek_at(&self, offset: usize) -> &Token {
        &self.tokens[(self.pos + offset).min(self.tokens.len() - 1)]
    }

    fn advance(&mut self) -> Token {
        let token = self.tokens[self.pos.min(self.tokens.len() - 1)].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        token
    }

    fn skip_newlines(&mut self) {
        while self.peek_kind() == TokenKind::Newline {
            self.advance();
        }
    }

    fn skip_expression_newlines(&mut self) {
        if self.peek_kind() != TokenKind::Newline {
            return;
        }
        let previous = self.tokens[..self.pos]
            .iter()
            .rev()
            .find(|token| !matches!(token.kind, TokenKind::Indent(_)));
        let previous_allows_continuation = previous.is_some_and(|token| {
            matches!(
                token.kind,
                TokenKind::LParen
                    | TokenKind::LBracket
                    | TokenKind::LBrace
                    | TokenKind::Comma
                    | TokenKind::Colon
                    | TokenKind::Assign
                    | TokenKind::Plus
                    | TokenKind::Minus
                    | TokenKind::Star
                    | TokenKind::Slash
                    | TokenKind::Percent
                    | TokenKind::DoubleStar
                    | TokenKind::Eq
                    | TokenKind::Ne
                    | TokenKind::Lt
                    | TokenKind::Le
                    | TokenKind::Gt
                    | TokenKind::Ge
            ) || (token.kind == TokenKind::Ident
                && matches!(token.text.as_str(), "and" | "or" | "in" | "not" | "if"))
        });
        let mut next = self.pos;
        while self.tokens[next].kind == TokenKind::Newline {
            next += 1;
        }
        let inside_delimiter_group = self.inside_delimiter_group();
        let next_allows_continuation = matches!(
            self.tokens[next].kind,
            TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Star
                | TokenKind::Slash
                | TokenKind::Percent
                | TokenKind::DoubleStar
                | TokenKind::Eq
                | TokenKind::Ne
                | TokenKind::Lt
                | TokenKind::Le
                | TokenKind::Gt
                | TokenKind::Ge
        ) || (inside_delimiter_group
            && matches!(
                self.tokens[next].kind,
                TokenKind::LParen
                    | TokenKind::LBracket
                    | TokenKind::Dot
                    | TokenKind::RParen
                    | TokenKind::RBracket
                    | TokenKind::RBrace
            ))
            || (self.tokens[next].kind == TokenKind::Ident
                && matches!(
                    self.tokens[next].text.as_str(),
                    "and" | "or" | "in" | "not" | "else"
                ))
            || (inside_delimiter_group
                && self.tokens[next].kind == TokenKind::Ident
                && matches!(self.tokens[next].text.as_str(), "if" | "for"));
        if previous_allows_continuation || next_allows_continuation {
            self.skip_newlines();
        }
    }

    fn is_ident(&self, text: &str) -> bool {
        self.peek_kind() == TokenKind::Ident && self.peek().text == text
    }

    fn expect_ident(&mut self, what: &str) -> Result<String, ()> {
        if self.peek_kind() == TokenKind::Ident {
            Ok(self.advance().text)
        } else {
            self.error_at_current(format!("expected {what}"));
            Err(())
        }
    }

    fn expect(&mut self, kind: TokenKind, what: &str) -> Result<Token, ()> {
        if self.peek_kind() == kind {
            Ok(self.advance())
        } else {
            self.error_at_current(format!("expected {what}"));
            Err(())
        }
    }

    fn error_at_current(&mut self, message: String) {
        let span = self.peek().span;
        self.errors.push(OpyError::at("parse-error", message, span));
    }

    // ---- program ----

    fn parse_program(&mut self) -> Program {
        let mut declarations = Vec::new();
        let mut rules = Vec::new();
        let mut top_level = Vec::new();
        loop {
            self.skip_newlines();
            if self.peek_kind() == TokenKind::Eof {
                break;
            }
            let rule_prefix = if self.peek_kind() == TokenKind::RulePrefixMarker {
                Some(self.advance().text)
            } else {
                None
            };
            let declaration_count = declarations.len();
            let rule_count = rules.len();
            let ok = self.parse_top_level(&mut declarations, &mut rules, rule_prefix);
            if ok {
                if declarations.len() > declaration_count {
                    top_level.push(TopLevel::Declaration(
                        declarations
                            .last()
                            .expect("declaration was appended")
                            .clone(),
                    ));
                } else if rules.len() > rule_count {
                    top_level.push(TopLevel::Rule(
                        rules.last().expect("rule was appended").clone(),
                    ));
                }
            }
            if !ok {
                self.recover_line();
            }
        }
        Program {
            declarations,
            rules,
            top_level,
            settings: None,
        }
    }

    fn parse_top_level(
        &mut self,
        declarations: &mut Vec<Decl>,
        rules: &mut Vec<RuleEntry>,
        rule_prefix: Option<String>,
    ) -> bool {
        let token = self.peek();
        if token.kind == TokenKind::Ident {
            match token.text.as_str() {
                "rule" => return self.parse_rule(rules, rule_prefix),
                "def" => return self.parse_def(rules, rule_prefix),
                "globalvar" => return self.parse_variable(declarations, true),
                "playervar" => return self.parse_variable(declarations, false),
                "subroutine" => return self.parse_subroutine(declarations),
                "enum" => return self.parse_enum(declarations),
                "macro" => return self.parse_macro(declarations),
                _ => {}
            }
        }
        self.error_at_current(format!(
            "expected a top-level declaration (rule/def/globalvar/playervar/subroutine/enum/macro) but found '{}'",
            token.text
        ));
        false
    }

    /// Skip to the end of the current line (error recovery).
    fn recover_line(&mut self) {
        while self.peek_kind() != TokenKind::Newline && self.peek_kind() != TokenKind::Eof {
            self.advance();
        }
    }

    /// The indentation of the next non-empty line, which must exceed
    /// `line_indent` (an indented block follows the colon).
    fn block_indent(&mut self, line_indent: u32) -> Option<u32> {
        self.skip_newlines();
        if self.peek_kind() == TokenKind::Eof {
            self.error_at_current("expected an indented block".to_string());
            return None;
        }
        let indent = self.peek().span.start.col;
        if indent <= line_indent {
            self.error_at_current("expected an indented block after ':'".to_string());
            return None;
        }
        Some(indent)
    }

    fn expect_statement_end(&mut self, what: &str) -> Result<(), ()> {
        let continued_line = self
            .tokens
            .get(self.pos.saturating_sub(1))
            .is_some_and(|previous| self.peek().span.start.line > previous.span.end.line);
        if matches!(self.peek_kind(), TokenKind::Newline | TokenKind::Eof) || continued_line {
            Ok(())
        } else {
            self.error_at_current(format!("expected the end of {what}"));
            Err(())
        }
    }
}

/// Parse one f-string expression fragment and shift its local token spans
/// into the original source file.
pub(crate) fn parse_expression_fragment(
    text: &str,
    file: u32,
    origin: Position,
) -> Result<Expr, OpyError> {
    let mut tokens = crate::lexer::lex(crate::lexer::LexInput {
        file_id: file,
        text,
    })?;
    for token in &mut tokens {
        token.span = shift_span(token.span, origin);
    }
    let mut parser = Parser {
        tokens: &tokens,
        pos: 0,
        allow_macro_redeclaration: false,
        errors: Vec::new(),
    };
    let expression = parser.parse_expr().map_err(|()| {
        parser.errors.first().cloned().unwrap_or_else(|| {
            OpyError::at(
                "parse-error",
                "invalid f-string expression",
                Span::new(file, origin, origin),
            )
        })
    })?;
    if parser.peek_kind() != TokenKind::Eof {
        parser.error_at_current("unexpected tokens in f-string interpolation".to_string());
    }
    parser.errors.into_iter().next().map_or(Ok(expression), Err)
}

fn shift_span(span: Span, origin: Position) -> Span {
    fn shift(position: Position, origin: Position) -> Position {
        Position::new(
            origin.line + position.line.saturating_sub(1),
            if position.line == 1 {
                origin.col + position.col.saturating_sub(1)
            } else {
                position.col
            },
        )
    }
    Span::new(
        span.file,
        shift(span.start, origin),
        shift(span.end, origin),
    )
}

fn decode_string_escape(character: char) -> char {
    match character {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        '\\' => '\\',
        '"' => '"',
        '\'' => '\'',
        other => other,
    }
}

fn is_string_modifier(text: &str) -> bool {
    matches!(text, "f" | "w" | "l" | "b" | "c" | "t")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::{LexInput, lex};

    fn parse_ok(text: &str) -> Program {
        let tokens = lex(LexInput { file_id: 0, text }).unwrap();
        let output = parse(&tokens);
        assert!(
            output.errors.is_empty(),
            "unexpected errors: {:?}",
            output.errors
        );
        output.program.unwrap()
    }

    fn parse_err(text: &str) -> Vec<OpyError> {
        let tokens = lex(LexInput { file_id: 0, text }).unwrap();
        parse(&tokens).errors
    }

    #[test]
    fn parses_basic_rule() {
        let program = parse_ok("rule \"setup\":\n    @Event global\n    disableInspector()\n");
        assert_eq!(program.rules.len(), 1);
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        assert_eq!(rule.name, "setup");
        assert_eq!(rule.event.name, "global");
        assert_eq!(rule.actions.len(), 1);
    }

    #[test]
    fn parses_power_augmented_assignment() {
        // The pinned OverPy 9.7.10 reference accepts `**=` as the power
        // augmented assignment (`a **= b` ⇔ `a = a ** b`).
        let program =
            parse_ok("globalvar a\nrule \"r\":\n    @Event global\n    a = 2\n    a **= 3\n");
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        let Stmt::Assign {
            value,
            target: assigned_target,
            ..
        } = &rule.actions[1]
        else {
            panic!("expected an assignment");
        };
        let Expr::Binary {
            op, left, right, ..
        } = value
        else {
            panic!("expected a binary modification, got {value:?}");
        };
        assert_eq!(op, "**");
        assert!(matches!(&**left, Expr::Name { .. }));
        assert!(matches!(
            assigned_target,
            Expr::Name { name, .. } if name == "a"
        ));
        assert!(matches!(right.as_ref(), Expr::Number { .. }));
    }

    #[test]
    fn parses_postfix_increment_and_decrement_as_modifications() {
        let program =
            parse_ok("globalvar value\nrule \"r\":\n    @Event global\n    value++\n    value--\n");
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected a rule");
        };
        for (statement, expected_op) in [(&rule.actions[0], "+"), (&rule.actions[1], "-")] {
            let Stmt::Assign { target, value, .. } = statement else {
                panic!("expected a postfix assignment");
            };
            let Expr::Binary {
                op, left, right, ..
            } = value
            else {
                panic!("expected a synthetic modification value");
            };
            assert_eq!(op, expected_op);
            let Expr::Name {
                name: left_name, ..
            } = left.as_ref()
            else {
                panic!("expected the target to be the modification's left operand");
            };
            let Expr::Name {
                name: target_name, ..
            } = target
            else {
                panic!("expected a name target");
            };
            assert_eq!(left_name, target_name);
            assert!(
                matches!(right.as_ref(), Expr::Number { value, text, .. } if *value == 1.0 && text == "1")
            );
        }
    }

    #[test]
    fn rejects_prefix_increment_and_embedded_postfix_forms() {
        for source in [
            "globalvar value\nrule \"r\":\n    @Event global\n    ++value\n",
            "globalvar value\nrule \"r\":\n    @Event global\n    value++++\n",
        ] {
            let errors = parse_err(source);
            assert!(!errors.is_empty());
            assert!(errors.iter().all(|error| error.code == "parse-error"));
            assert!(errors.iter().all(|error| error.span.is_some()));
        }
    }

    #[test]
    fn preserves_consecutive_unary_minus_expressions() {
        let source = concat!(
            "globalvar value = 0\n",
            "globalvar B = 1\n",
            "rule \"r\":\n",
            "    @Event global\n",
            "    value = --1\n",
            "    value = --B\n",
            "    value = B--1\n",
        );
        parse_ok(source);
    }

    #[test]
    fn parses_control_flow() {
        let program = parse_ok(
            "globalvar index = 0\n\nrule \"r\":\n    @Event global\n    for index in range(3):\n        if index == 0:\n            debug(index)\n        elif index == 1:\n            debug(index)\n        else:\n            debug(index)\n    while index < 3:\n        index += 1\n        wait()\n",
        );
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!();
        };
        assert!(matches!(rule.actions[0], Stmt::For { .. }));
        let Stmt::For { body, .. } = &rule.actions[0] else {
            panic!();
        };
        let Stmt::If {
            branches, r#else, ..
        } = &body[0]
        else {
            panic!();
        };
        assert_eq!(branches.len(), 2);
        assert!(r#else.is_some());
        let Stmt::While { body, .. } = &rule.actions[1] else {
            panic!();
        };
        assert_eq!(body.len(), 2);
    }

    #[test]
    fn parses_source_statement_surface() {
        let program = parse_ok(concat!(
            "globalvar value\n",
            "rule \"r\":\n",
            "    @Event global\n",
            "    del value[1]\n",
            "    value min= 2\n",
            "    value max= 3\n",
            "    while value < 4:\n",
            "        continue\n",
            "    goto RULE_START\n",
            "    goto target\n",
            "    goto loc + value\n",
            "    target:\n",
        ));
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        assert!(matches!(rule.actions[0], Stmt::Delete { .. }));
        for (statement, expected) in [(&rule.actions[1], "min"), (&rule.actions[2], "max")] {
            let Stmt::Assign { value, .. } = statement else {
                panic!("expected augmented assignment");
            };
            assert!(matches!(value, Expr::Binary { op, .. } if op == expected));
        }
        let Stmt::While { body, .. } = &rule.actions[3] else {
            panic!("expected while");
        };
        assert!(matches!(body.as_slice(), [Stmt::Continue { .. }]));
        assert!(matches!(
            &rule.actions[4],
            Stmt::Goto {
                label: None,
                offset: None,
                rule_start: true,
                ..
            }
        ));
        assert!(matches!(
            &rule.actions[5],
            Stmt::Goto {
                label: Some(label),
                offset: None,
                rule_start: false,
                ..
            } if label == "target"
        ));
        assert!(matches!(
            &rule.actions[6],
            Stmt::Goto {
                label: None,
                offset: Some(_),
                rule_start: false,
                ..
            }
        ));
        assert!(matches!(&rule.actions[7], Stmt::Label { name, .. } if name == "target"));
    }

    #[test]
    fn rejects_invalid_source_statement_forms() {
        for source in [
            "rule \"r\":\n    @Event global\n    del value\n",
            "rule \"r\":\n    @Event global\n    goto\n",
            "rule \"r\":\n    @Event global\n    goto loc\n",
            "rule \"r\":\n    @Event global\n    goto target extra\n",
            "rule \"r\":\n    @Event global\n    continue now\n",
            "rule \"r\":\n    @Event global\n    A = 1; A = 2\n",
        ] {
            let errors = parse_err(source);
            assert!(!errors.is_empty(), "invalid form parsed: {source}");
            assert!(errors.iter().all(|error| error.code == "parse-error"));
            assert!(errors.iter().all(|error| error.span.is_some()));
        }
    }

    #[test]
    fn parses_syntax_constructs() {
        let program = parse_ok(
            "globalvar x\nrule \"r\":\n    @Event global\n    switch x:\n        case 0x10:\n            x = 1 in [1, 2]\n        default:\n            do:\n                x = {\"x\": 1}[\"x\"]\n            while x not in [2, 3]\n    x = [value * 2 for value, index in [1, 2] if value > index]\n    x = sorted([1, 2], key=lambda value: value)\n    x = w\"wide\"\n",
        );
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        assert!(matches!(rule.actions[0], Stmt::Switch { .. }));
        assert!(matches!(rule.actions[1], Stmt::Assign { .. }));
    }

    #[test]
    fn rejects_incomplete_do_while_and_dictionary_entries() {
        let errors = parse_err(
            "rule \"r\":\n    @Event global\n    do:\n        pass\n    while\n    x = {\"x\"}\n",
        );
        assert!(!errors.is_empty());
        assert!(errors.iter().all(|error| error.code == "parse-error"));
    }

    #[test]
    fn parses_explicit_enum_member_values() {
        let program = parse_ok("enum EventType:\n    BUFF = 0\n    DEBUFF\n    MECH = 2\n");
        let Decl::Enum { members, .. } = &program.declarations[0] else {
            panic!("expected enum");
        };
        assert_eq!(
            members
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["BUFF", "DEBUFF", "MECH"]
        );
    }

    #[test]
    fn parses_multi_line_array() {
        let program = parse_ok(
            "globalvar p\nrule \"r\":\n    @Event global\n    p = [\n        vect(1, 0, 0),\n        vect(2, 0, 0),\n    ]\n",
        );
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!();
        };
        let Stmt::Assign { value, .. } = &rule.actions[0] else {
            panic!();
        };
        let Expr::Array { elements, .. } = value else {
            panic!("expected array, got {value:?}");
        };
        assert_eq!(elements.len(), 2);
    }

    #[test]
    fn missing_colon_is_a_structured_error() {
        let errors = parse_err("rule \"x\"\n    @Event global\n");
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, "parse-error");
        assert!(errors[0].span.is_some());
    }

    #[test]
    fn def_and_macro_parse() {
        let program = parse_ok(
            "subroutine showStatus\n\nmacro VERSION = \"1.4.3\"\n\ndef showStatus():\n    print(\"hi\")\n\nmacro double(value):\n    value + value\n",
        );
        assert_eq!(program.declarations.len(), 3);
        assert!(matches!(program.declarations[1], Decl::Constant { .. }));
        assert!(matches!(program.declarations[2], Decl::Macro { .. }));
        let Decl::Macro { args, body, .. } = &program.declarations[2] else {
            panic!();
        };
        assert_eq!(args, &vec!["value".to_string()]);
        assert_eq!(body.len(), 1);
    }

    #[test]
    fn macro_and_enum_redeclarations_are_checked_at_ast_surfaces() {
        let text = "enum Kind:\n    First\n    First\nmacro helper():\n    pass\nmacro helper():\n    pass\n";
        let errors = parse_err(text);
        assert_eq!(
            errors
                .iter()
                .filter(|error| error.code == "macro-redeclaration")
                .count(),
            2
        );

        let tokens = lex(LexInput { file_id: 0, text }).unwrap();
        let output = parse_with_options(&tokens, true);
        assert!(
            output.errors.is_empty(),
            "unexpected errors: {:?}",
            output.errors
        );
        assert!(output.program.is_some());
    }

    #[test]
    fn multiple_errors_are_reported() {
        let errors =
            parse_err("rule \"a\"\n    bad statement here\nrule \"b\"\n    @Event global\n");
        assert!(!errors.is_empty());
    }

    #[test]
    fn precedence_parses_python_like() {
        let program = parse_ok("globalvar x\nrule \"r\":\n    @Event global\n    x = 1 + 2 * 3\n");
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!();
        };
        let Stmt::Assign { value, .. } = &rule.actions[0] else {
            panic!();
        };
        let Expr::Binary {
            op, left, right, ..
        } = value
        else {
            panic!();
        };
        assert_eq!(op, "+");
        let Expr::Binary { op: inner, .. } = right.as_ref() else {
            panic!();
        };
        assert_eq!(inner, "*");
        assert!(matches!(left.as_ref(), Expr::Number { .. }));
    }

    #[test]
    fn parses_right_associative_conditional_expressions() {
        let program = parse_ok(
            "rule \"r\":\n    @Event global\n    debug(1 if true else 2 if false else 3)\n",
        );
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected a rule");
        };
        let Stmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected an expression statement");
        };
        let Expr::Call { args, .. } = expr else {
            panic!("expected a call");
        };
        let Expr::Conditional {
            then_value,
            condition,
            else_value,
            span,
        } = &args[0].value
        else {
            panic!("expected a conditional expression");
        };
        assert!(matches!(then_value.as_ref(), Expr::Number { value, .. } if *value == 1.0));
        assert!(matches!(condition.as_ref(), Expr::Bool { value: true, .. }));
        assert!(matches!(
            else_value.as_ref(),
            Expr::Conditional { then_value, condition, else_value, .. }
                if matches!(then_value.as_ref(), Expr::Number { value, .. } if *value == 2.0)
                    && matches!(condition.as_ref(), Expr::Bool { value: false, .. })
                    && matches!(else_value.as_ref(), Expr::Number { value, .. } if *value == 3.0)
        ));
        assert_eq!(span.start.line, 3);
        assert_eq!(span.start.col, 11);
    }

    #[test]
    fn parses_parenthesized_nested_conditional_and_rejects_missing_else() {
        let program = parse_ok(
            "rule \"r\":\n    @Event global\n    debug((1 if true else 2) if false else 3)\n",
        );
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected a rule");
        };
        let Stmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected an expression statement");
        };
        let Expr::Call { args, .. } = expr else {
            panic!("expected a call");
        };
        assert!(matches!(
            &args[0].value,
            Expr::Conditional {
                then_value,
                condition,
                else_value,
                ..
            } if matches!(then_value.as_ref(), Expr::Conditional { .. })
                && matches!(condition.as_ref(), Expr::Bool { value: false, .. })
                && matches!(else_value.as_ref(), Expr::Number { value, .. } if *value == 3.0)
        ));

        let errors = parse_err("rule \"r\":\n    @Event global\n    debug(1 if true)\n");
        assert_eq!(errors[0].code, "parse-error");
        assert!(errors[0].message.contains("expected `else`"));
    }

    #[test]
    fn parses_receiver_calls() {
        // `eventPlayer.setMoveSpeed(100)` is a receiver call: postfix `.`
        // member access followed by call arguments (#104).
        let program =
            parse_ok("rule \"r\":\n    @Event eachPlayer\n    eventPlayer.setMoveSpeed(100)\n");
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        let Stmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected expression statement, got {:?}", rule.actions[0]);
        };
        let Expr::ReceiverCall {
            receiver,
            name,
            args,
            ..
        } = &expr
        else {
            panic!("expected receiver call, got {expr:?}");
        };
        assert_eq!(name, "setMoveSpeed");
        assert!(
            matches!(receiver.as_ref(), Expr::Name { name, .. } if name == "eventPlayer"),
            "receiver must be the eventPlayer name"
        );
        assert_eq!(args.len(), 1);
        assert!(args[0].keyword.is_none(), "positional argument");
        assert!(matches!(&args[0].value, Expr::Number { .. }));
    }

    #[test]
    fn parses_keyword_arguments_with_name_spans() {
        // `name = expr` call arguments are keyword arguments carrying the
        // name token's exact span (issue #110); comparisons stay positional.
        let program =
            parse_ok("rule \"r\":\n    @Event global\n    wait(time=1)\n    debug(g == 1)\n");
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        let Stmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected expression statement");
        };
        let Expr::Call { args, .. } = expr else {
            panic!("expected a call, got {expr:?}");
        };
        let (keyword, span) = args[0].keyword.as_ref().expect("keyword argument");
        assert_eq!(keyword, "time");
        assert_eq!(span.start.line, 3);
        assert!(matches!(&args[0].value, Expr::Number { .. }));

        let Stmt::Expr { expr, .. } = &rule.actions[1] else {
            panic!("expected expression statement");
        };
        let Expr::Call { args, .. } = expr else {
            panic!("expected a call, got {expr:?}");
        };
        assert!(args[0].keyword.is_none(), "comparisons are not keywords");
        assert!(matches!(&args[0].value, Expr::Binary { .. }));
    }

    #[test]
    fn adjacent_string_literals_concatenate_and_preserve_span() {
        let program = parse_ok("rule \"r\":\n    @Event global\n    debug(\"one\" \"two\")\n");
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        let Stmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected expression statement");
        };
        let Expr::Call { args, .. } = expr else {
            panic!("expected call");
        };
        let Expr::String { value, span } = &args[0].value else {
            panic!("expected concatenated string");
        };
        assert_eq!(value, "onetwo");
        assert_eq!(span.start.line, 3);
        assert_eq!(span.start.col, 11);
        assert_eq!(span.end.col, 22);
    }

    #[test]
    fn multiline_adjacent_string_literals_concatenate_inside_group() {
        let program =
            parse_ok("rule \"r\":\n    @Event global\n    debug(\"one\"\n        \"two\")\n");
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        let Stmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected expression statement");
        };
        let Expr::Call { args, .. } = expr else {
            panic!("expected call");
        };
        assert!(matches!(
            &args[0].value,
            Expr::String { value, .. } if value == "onetwo"
        ));
    }

    #[test]
    fn newline_outside_group_keeps_adjacent_literals_as_statements() {
        let program = parse_ok("rule \"r\":\n    @Event global\n    \"one\"\n    \"two\"\n");
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        assert_eq!(rule.actions.len(), 2);
    }

    #[test]
    fn non_name_keyword_lhs_is_a_parse_error() {
        // `f(1 = 2)` is not a call argument form; rejected explicitly.
        let errors = parse_err("rule \"r\":\n    @Event global\n    debug(1 = 2)\n");
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, "parse-error");
    }

    #[test]
    fn parses_member_call_on_call_result() {
        // `getPlayersInRadius(...).setStatusEffect(...)`: member access
        // followed by call arguments on a call result stays a receiver call.
        let program = parse_ok(
            "rule \"r\":\n    @Event eachPlayer\n    getPlayersInRadius(eventPlayer, 10).setStatusEffect(eventPlayer, 30)\n",
        );
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        let Stmt::Expr { expr, .. } = &rule.actions[0] else {
            panic!("expected expression statement");
        };
        let Expr::ReceiverCall {
            receiver,
            name,
            args,
            ..
        } = &expr
        else {
            panic!("expected receiver call, got {expr:?}");
        };
        assert_eq!(name, "setStatusEffect");
        assert!(
            matches!(receiver.as_ref(), Expr::Call { name, .. } if name == "getPlayersInRadius"),
            "receiver must be the preceding call"
        );
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn member_without_call_is_not_a_call() {
        // `eventPlayer.moveSpeed` alone (no parentheses) stays a member
        // access; only a following `(` turns it into a receiver call.
        let program =
            parse_ok("rule \"r\":\n    @Event eachPlayer\n    x = eventPlayer.moveSpeed\n");
        let RuleEntry::Rule(rule) = &program.rules[0] else {
            panic!("expected rule");
        };
        let Stmt::Assign { value, .. } = &rule.actions[0] else {
            panic!("expected assignment");
        };
        assert!(matches!(
            &value,
            Expr::Member { member, .. } if member == "moveSpeed"
        ));
    }

    #[test]
    fn parses_advanced_rule_annotations_with_source_arguments() {
        let program = parse_ok(
            "subroutine helper\ndef helper():\n    @Name \"renamed\"\n    @SuppressWarnings unusedVariable\n    pass\nrule \"r\":\n    @Event eachPlayer\n    @Team 1\n    @Hero dmon\n    @Disabled\n    @Delimiter\n    @NewPage \"Page\"\n    @SuppressWarnings unusedVariable\n    pass\n",
        );
        let RuleEntry::SubroutineDef { annotations, .. } = &program.rules[0] else {
            panic!("expected subroutine");
        };
        assert_eq!(annotations.len(), 2);
        let RuleEntry::Rule(rule) = &program.rules[1] else {
            panic!("expected rule");
        };
        assert!(rule.disabled);
        assert!(rule.delimiter);
        assert_eq!(rule.new_page.as_deref(), Some("Page"));
        assert_eq!(rule.annotations.len(), 7);
        assert_eq!(rule.annotations[1].args[0].text, "1");
        assert_eq!(rule.annotations[2].args[0].text, "dmon");
    }
}
