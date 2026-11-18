use super::*;

/// Workshop setting values keyed by their canonical Workshop name; the OPY
/// factory that lowers back to the same call takes the same arguments.
const SETTING_FACTORIES: &[(&str, &str)] = &[
    ("workshopSettingToggle", "createWorkshopSettingBool"),
    ("workshopSettingInteger", "createWorkshopSettingInt"),
    ("workshopSettingCombo", "createWorkshopSettingEnum"),
    ("createWorkshopSettingFloat", "createWorkshopSettingFloat"),
    ("createWorkshopSettingHero", "createWorkshopSettingHero"),
];

impl<'a> Emitter<'a> {
    /// Array literals, Workshop setting values, event-player keywords and
    /// `exclude`: the value calls whose OPY form is not a catalog call.
    pub(super) fn emit_literal_value(
        &mut self,
        name: &str,
        args: &[Value],
        span: Option<Span>,
    ) -> bool {
        if name == "array" {
            self.out.push('[');
            self.emit_value_list(args, span);
            self.out.push(']');
            return true;
        }
        if matches!(name, "attacker" | "victim" | "healer" | "healee") && args.is_empty() {
            self.out.push_str(name);
            return true;
        }
        if let ("removeFromArray", [array, removed]) = (name, args) {
            self.emit_parenthesized(array, span);
            self.out.push_str(".exclude(");
            self.emit_value(removed, span);
            self.out.push(')');
            return true;
        }
        let Some((_, factory)) = SETTING_FACTORIES.iter().find(|(n, _)| *n == name) else {
            return false;
        };
        self.out.push_str(factory);
        self.out.push('(');
        self.emit_value_list(args, span);
        self.out.push(')');
        true
    }

    /// The right-hand side of a Set line, newline included.
    /// `x = x.exclude(v)` recompiles to the Modify form, so a Set whose
    /// `removeFromArray` operand is its own target cannot round-trip.
    pub(super) fn set_rhs(&mut self, target: &str, value: &Value, span: Option<Span>) {
        if let Value::Call { name, args } = value
            && name == "removeFromArray"
            && args.len() == 2
            && self.render_value_as(&args[0], span, |this, v, s| this.emit_value(v, s)) == target
        {
            self.issue(
                "unsupported-value-call",
                format!(
                    "value call 'removeFromArray' on its own assignment target '{target}' \
                 recompiles to a modify action on the OPY surface"
                ),
                span,
            );
            return;
        }
        self.emit_value(value, span);
        self.out.push('\n');
    }

    fn emit_value_list(&mut self, args: &[Value], span: Option<Span>) {
        for (index, arg) in args.iter().enumerate() {
            if index > 0 {
                self.out.push_str(", ");
            }
            self.emit_value(arg, span);
        }
    }

    /// The callback binders `currentArrayElement` / `currentArrayIndex` read
    /// the innermost lambda parameters.
    pub(super) fn emit_array_binder(
        &mut self,
        name: &str,
        args: &[Value],
        span: Option<Span>,
    ) -> bool {
        if !args.is_empty() {
            return false;
        }
        let message = match (name, self.lambda_binders.last()) {
            ("currentArrayElement", Some((element, _))) => {
                self.out.push_str(element);
                return true;
            }
            ("currentArrayIndex", Some((_, Some(index)))) => {
                self.out.push_str(index);
                return true;
            }
            ("currentArrayElement", None) => {
                "currentArrayElement outside an array callback has no OPY binder"
            }
            ("currentArrayIndex", Some(_)) => {
                "currentArrayIndex used but the callback has no index binder"
            }
            ("currentArrayIndex", None) => {
                "currentArrayIndex outside an array callback has no OPY binder"
            }
            _ => return false,
        };
        self.issue("unsupported-value-call", message, span);
        true
    }
}
