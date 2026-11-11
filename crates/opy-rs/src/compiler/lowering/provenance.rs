//! Provenance of lowered Workshop nodes.
//!
//! `Lowering::program` carries definition-site spans, the same as a compile
//! without macro attribution. When macro expansion attributed a node to its
//! invocation site, the provenance assignment is also logged with the
//! attributed spans; replaying the log onto a copy of the program yields the
//! program the source map is extracted from.

use super::*;

/// A Workshop node that carries provenance.
#[derive(Debug, Clone, Copy)]
pub(super) enum ProvenanceNode {
    Rule(usize),
    Condition {
        rule: usize,
        condition: usize,
    },
    Action {
        rule: usize,
        action: usize,
    },
    ActionArgument {
        rule: usize,
        action: usize,
        argument: usize,
    },
    GlobalVariable(usize),
    PlayerVariable(usize),
    Subroutine(usize),
}

type WorkshopSpan = Option<workshop_rs::source::Span>;

impl ProvenanceNode {
    fn set(
        self,
        program: &mut Program,
        span: WorkshopSpan,
        name_span: WorkshopSpan,
    ) -> Result<(), workshop_rs::program::SourceMappingError> {
        match self {
            Self::Rule(rule) => program.set_rule_span(rule, span),
            Self::Condition { rule, condition } => {
                program.set_condition_span(rule, condition, span)
            }
            Self::Action { rule, action } => program.set_action_span(rule, action, span),
            Self::ActionArgument {
                rule,
                action,
                argument,
            } => program.set_action_argument_span(rule, action, argument, span),
            Self::GlobalVariable(index) => {
                program.set_global_variable_spans(index, span, name_span)
            }
            Self::PlayerVariable(index) => {
                program.set_player_variable_spans(index, span, name_span)
            }
            Self::Subroutine(index) => program.set_subroutine_spans(index, span, name_span),
        }
    }
}

/// A provenance assignment with invocation-site spans.
pub(super) struct SiteEdit {
    node: ProvenanceNode,
    span: WorkshopSpan,
    name_span: WorkshopSpan,
}

impl<'a> Lowering<'a> {
    pub(super) fn workshop_span(
        &self,
        span: Option<HirSpan>,
    ) -> Result<WorkshopSpan, IntegrationError> {
        let Some(span) = span else {
            return Ok(None);
        };
        if !self.hir.files.iter().any(|file| file.id == span.file) {
            return Err(IntegrationError::new(
                "source-file",
                format!("HIR span references unknown source file id {}", span.file),
                Some(span),
            ));
        }
        Ok(Some(workshop_rs::source::Span::new(
            workshop_rs::source::FileId::from_index(span.file as usize),
            workshop_rs::source::Position::new(span.start.line, span.start.col),
            workshop_rs::source::Position::new(span.end.line, span.end.col),
        )))
    }

    /// Set the provenance of `node` from the definition-site spans, and log
    /// the attributed assignment when either span carries a site.
    pub(super) fn set_provenance(
        &mut self,
        node: ProvenanceNode,
        span: Option<HirSpan>,
        name_span: Option<HirSpan>,
    ) -> Result<(), IntegrationError> {
        let definition = HirSpan::definition(span);
        // A compile without attribution never records a span-less argument.
        if definition.is_some() || !matches!(node, ProvenanceNode::ActionArgument { .. }) {
            let definition = [
                self.workshop_span(definition)?,
                self.workshop_span(HirSpan::definition(name_span))?,
            ];
            node.set(&mut self.program, definition[0], definition[1])
                .map_err(|error| {
                    IntegrationError::new("provenance", error.to_string(), span.or(name_span))
                })?;
        }
        if [span, name_span]
            .iter()
            .flatten()
            .any(|span| span.site.is_some())
        {
            self.site_edits.push(SiteEdit {
                node,
                span: self.workshop_span(span.map(HirSpan::attributed))?,
                name_span: self.workshop_span(name_span.map(HirSpan::attributed))?,
            });
        }
        Ok(())
    }

    /// The program whose provenance attributes macro-expanded nodes to their
    /// invocation site, or `None` when no node was attributed.
    pub(in crate::compiler) fn attributed_program(
        &self,
    ) -> Result<Option<Program>, IntegrationError> {
        if self.site_edits.is_empty() {
            return Ok(None);
        }
        let mut program = self.program.clone();
        for edit in &self.site_edits {
            edit.node
                .set(&mut program, edit.span, edit.name_span)
                .map_err(|error| IntegrationError::new("provenance", error.to_string(), None))?;
        }
        Ok(Some(program))
    }

    pub(super) fn append_rule(
        &mut self,
        rule_value: workshop_rs::Rule,
        actions: &[ActionId],
        span: Option<HirSpan>,
        condition_spans: impl IntoIterator<Item = Option<HirSpan>>,
    ) -> Result<(), IntegrationError> {
        let rule = self.program.rules.len();
        self.program.rules.push(rule_value);
        self.set_provenance(ProvenanceNode::Rule(rule), span, None)?;
        for (condition, span) in condition_spans.into_iter().enumerate() {
            self.set_provenance(ProvenanceNode::Condition { rule, condition }, span, None)?;
        }
        for (action, (span, argument_spans)) in
            self.action_provenance(actions).into_iter().enumerate()
        {
            self.set_provenance(ProvenanceNode::Action { rule, action }, span, None)?;
            for (argument, span) in argument_spans.into_iter().enumerate() {
                if span.is_some() {
                    let node = ProvenanceNode::ActionArgument {
                        rule,
                        action,
                        argument,
                    };
                    self.set_provenance(node, span, None)?;
                }
            }
        }
        Ok(())
    }
}
