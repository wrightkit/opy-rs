use super::*;

impl<'a> Lowering<'a> {
    pub(super) fn fold_format_constants<'b>(
        &self,
        format: &'b hir::Expr,
    ) -> (String, Vec<&'b hir::Expr>) {
        let optimized = self.optimization_state_at(format.span()).enabled;
        self.fold_constants(format, optimized)
    }

    /// Translated formats always fold: their replacement fallback passes the
    /// arguments to `stringReplace`, which rejects an unfolded numeric
    /// expression.
    pub(super) fn fold_translated_format<'b>(
        &self,
        format: &'b hir::Expr,
    ) -> (String, Vec<&'b hir::Expr>) {
        self.fold_constants(format, true)
    }

    fn fold_constants<'b>(
        &self,
        format: &'b hir::Expr,
        optimized: bool,
    ) -> (String, Vec<&'b hir::Expr>) {
        let hir::Expr::Format { text, args, .. } = format else {
            unreachable!("format folding is only reached for format expressions")
        };
        let bindings = HashMap::new();
        let mut values = Vec::with_capacity(args.len());
        let mut dynamic_args = Vec::new();
        let mut dynamic_position = HashMap::new();
        for (index, arg) in args.iter().enumerate() {
            let mut stack = Vec::new();
            let value = optimized
                .then(|| crate::compile_time::evaluate(arg, &self.constants, &bindings, &mut stack))
                .flatten()
                .and_then(crate::compile_time::display);
            if value.is_none() {
                dynamic_position.insert(index, dynamic_args.len());
                dynamic_args.push(arg);
            }
            values.push(value);
        }

        let canonical = canonical_format_text(text);
        let mut output = String::with_capacity(canonical.len());
        let mut cursor = 0;
        while cursor < canonical.len() {
            let Some(open_rel) = canonical[cursor..].find('{') else {
                output.push_str(&canonical[cursor..]);
                break;
            };
            let open = cursor + open_rel;
            output.push_str(&canonical[cursor..open]);
            let Some(close_rel) = canonical[open + 1..].find('}') else {
                output.push_str(&canonical[open..]);
                break;
            };
            let close = open + 1 + close_rel;
            let marker = &canonical[open + 1..close];
            let Ok(index) = marker.parse::<usize>() else {
                output.push_str(&canonical[open..=close]);
                cursor = close + 1;
                continue;
            };
            if let Some(Some(value)) = values.get(index) {
                output.push_str(value);
            } else if let Some(position) = dynamic_position.get(&index) {
                output.push('{');
                output.push_str(&position.to_string());
                output.push('}');
            } else {
                output.push_str(&canonical[open..=close]);
            }
            cursor = close + 1;
        }
        (output, dynamic_args)
    }
}
