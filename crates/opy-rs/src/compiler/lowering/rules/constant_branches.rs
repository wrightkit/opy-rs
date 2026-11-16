use super::*;

impl Lowering<'_> {
    /// The pinned reference decides a constant condition while optimizing:
    /// a false `if`/`elif` branch and a false `while` vanish, and a true
    /// branch ends its chain, becoming the `else` (or the plain body when no
    /// condition precedes it).
    pub(super) fn constant_condition(
        &self,
        condition: ValueId,
        span: Option<HirSpan>,
    ) -> Option<bool> {
        let optimization = self.optimization_state_at(span.as_ref());
        if !optimization.enabled {
            return None;
        }
        OperatorOptimizer::new(self.compiler, optimization.strict)
            .constant_truth(&self.materialize_value(condition))
    }

    pub(super) fn push_folded_if_actions(
        &mut self,
        branches: Vec<(ValueId, Vec<ActionId>)>,
        mut else_body: Option<Vec<ActionId>>,
        span: Option<HirSpan>,
    ) -> Vec<ActionId> {
        let mut kept = Vec::new();
        for (condition, body) in branches {
            match self.constant_condition(condition, span) {
                Some(false) => {}
                Some(true) => {
                    else_body = Some(body);
                    break;
                }
                None => kept.push((condition, body)),
            }
        }
        if kept.is_empty() {
            return else_body.unwrap_or_default();
        }
        // A lone `if` whose body produces no actions — an empty or
        // `pass`-only body — emits nothing in the pinned reference,
        // while `elif`/`else` headers still emit when present (#516).
        if let [(_, body)] = kept.as_slice()
            && else_body.is_none()
            && body.is_empty()
        {
            return Vec::new();
        }
        self.push_if_actions(kept, else_body)
    }
}
