//! The optimization state that applies to a lowered node.

use super::*;

pub(super) struct OptimizationIndex {
    files: HashMap<u32, Vec<(hir::Position, OptimizationState)>>,
}

impl OptimizationIndex {
    pub(super) fn build(hir: &hir::Program) -> Self {
        let mut files: HashMap<u32, Vec<(hir::Position, OptimizationState)>> = HashMap::new();
        for directive in &hir.preprocessing.directives {
            let Some(span) = directive.span else {
                continue;
            };
            files
                .entry(span.file)
                .or_default()
                .push((span.start, directive.state.optimization.clone()));
        }
        Self { files }
    }

    fn state_at(&self, file: u32, position: hir::Position) -> Option<&OptimizationState> {
        let entries = self.files.get(&file)?;
        let index = entries.partition_point(|(start, _)| {
            start.line < position.line || (start.line == position.line && start.col <= position.col)
        });
        index.checked_sub(1).map(|index| &entries[index].1)
    }
}

impl<'a> Lowering<'a> {
    /// The optimization state where a node's source text is compiled: its
    /// macro invocation site when macro expansion recorded one, else its
    /// definition site. Upstream substitutes macro text at the call site, so
    /// the call site's directives apply. A span-less node takes the program
    /// state.
    pub(in crate::compiler) fn optimization_state_at(
        &self,
        span: Option<&HirSpan>,
    ) -> OptimizationState {
        #[cfg(test)]
        self.optimization_queries.borrow_mut().push(span.cloned());
        if let Some(state) = &self.optimization_override {
            return state.clone();
        }
        let Some(span) = span.map(|span| span.attributed()) else {
            return self.hir.preprocessing.optimization.clone();
        };
        self.optimization_index
            .state_at(span.file, span.start)
            .cloned()
            .or_else(|| {
                self.hir
                    .preprocessing
                    .source_file_initial_optimization
                    .get(&span.file)
                    .cloned()
            })
            .unwrap_or_else(|| self.hir.preprocessing.optimization.clone())
    }

    #[cfg(test)]
    pub(in crate::compiler) fn optimization_state_scan(
        &self,
        span: Option<&HirSpan>,
    ) -> OptimizationState {
        if let Some(state) = &self.optimization_override {
            return state.clone();
        }
        let Some(span) = span.map(|span| span.attributed()) else {
            return self.hir.preprocessing.optimization.clone();
        };
        let mut active = None;
        for directive in &self.hir.preprocessing.directives {
            let Some(directive_span) = directive.span else {
                continue;
            };
            if directive_span.file != span.file {
                continue;
            }
            if directive_span.start.line < span.start.line
                || (directive_span.start.line == span.start.line
                    && directive_span.start.col <= span.start.col)
            {
                active = Some(directive.state.optimization.clone());
            } else {
                break;
            }
        }
        active
            .or_else(|| {
                self.hir
                    .preprocessing
                    .source_file_initial_optimization
                    .get(&span.file)
                    .cloned()
            })
            .unwrap_or_else(|| self.hir.preprocessing.optimization.clone())
    }
}
