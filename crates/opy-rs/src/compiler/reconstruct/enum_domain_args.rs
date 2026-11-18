use super::*;

/// Workshop's name for a constant spelling shared by several enum domains;
/// the owning parameter's domain picks the member.
const AMBIGUOUS_ENUM_CALL: &str = "__ambiguous_enum";

impl<'a> Emitter<'a> {
    /// Checks an argument for a parameter with an enum domain and returns the
    /// value to emit: a context-resolved constant becomes the member of that
    /// domain. Enum literals must belong to the domain; any other value that
    /// can evaluate to a member (null, a call, a variable, an array of those)
    /// is kept as it is.
    pub(super) fn check_domain_argument<'v>(
        &mut self,
        entry: &Function,
        index: usize,
        domain: &str,
        arg: &'v Value,
        span: Option<Span>,
    ) -> Cow<'v, Value> {
        let arg = Self::resolve_ambiguous_member(domain, arg);
        match &*arg {
            Value::Enum { value_type, value } if value_type == domain => {
                if !self.enum_member_in_domain(domain, value) {
                    self.issue(
                        "unsupported-enum-member",
                        format!(
                            "argument {} of '{}' uses enum member '{domain}.{value}' \
                             which is outside the manifest's declared domain",
                            index + 1,
                            entry.id
                        ),
                        span,
                    );
                }
            }
            Value::Enum { value_type, .. } => {
                self.issue(
                    "unsupported-enum-domain-mismatch",
                    format!(
                        "argument {} of '{}' expects enum domain '{domain}' but \
                         the Workshop value carries '{value_type}'",
                        index + 1,
                        entry.id
                    ),
                    span,
                );
            }
            Value::Call { name, .. } if name == AMBIGUOUS_ENUM_CALL => {
                self.issue(
                    "unsupported-enum-domain-mismatch",
                    format!(
                        "argument {} of '{}' is a constant with no member in enum \
                         domain '{domain}'",
                        index + 1,
                        entry.id
                    ),
                    span,
                );
            }
            Value::Null
            | Value::Call { .. }
            | Value::GlobalVariable(_)
            | Value::PlayerVariable { .. } => {}
            Value::Array(elements) => {
                for element in elements {
                    self.check_domain_argument(entry, index, domain, element, span);
                }
            }
            _ => {
                self.issue(
                    "unsupported-enum-domain-mismatch",
                    format!(
                        "argument {} of '{}' expects an enum member of domain \
                         '{domain}'",
                        index + 1,
                        entry.id
                    ),
                    span,
                );
            }
        }
        arg
    }

    /// `__ambiguous_enum(spelling, [candidates])` stands for the candidate in
    /// `domain`, or for an array whose elements are such constants.
    fn resolve_ambiguous_member<'v>(domain: &str, arg: &'v Value) -> Cow<'v, Value> {
        match arg {
            Value::Call { name, args } if name == AMBIGUOUS_ENUM_CALL => {
                let candidate = match args.as_slice() {
                    [_, Value::Array(candidates)] => candidates.iter().find(|candidate| {
                        matches!(candidate, Value::Enum { value_type, .. } if value_type == domain)
                    }),
                    _ => None,
                };
                candidate.map_or(Cow::Borrowed(arg), |c| Cow::Owned(c.clone()))
            }
            Value::Array(elements) => {
                let resolved: Vec<Cow<'v, Value>> = elements
                    .iter()
                    .map(|element| Self::resolve_ambiguous_member(domain, element))
                    .collect();
                if resolved.iter().all(|e| matches!(e, Cow::Borrowed(_))) {
                    Cow::Borrowed(arg)
                } else {
                    Cow::Owned(Value::Array(
                        resolved.into_iter().map(Cow::into_owned).collect(),
                    ))
                }
            }
            _ => Cow::Borrowed(arg),
        }
    }

    /// Vector is the only enum domain whose members are arithmetic operands,
    /// so a shared constant there is the Vector member.
    pub(super) fn emit_arithmetic_operand(&mut self, operand: &Value, span: Option<Span>) {
        let operand = Self::resolve_ambiguous_member("Vector", operand);
        self.emit_value(&operand, span);
    }

    /// A shared constant outside an enum-domain argument has no context that
    /// picks a member; the refusal names it.
    pub(super) fn refuse_ambiguous_constant(
        &mut self,
        name: &str,
        args: &[Value],
        span: Option<Span>,
    ) -> bool {
        if name != AMBIGUOUS_ENUM_CALL {
            return false;
        }
        let spelling = match args.first() {
            Some(Value::String(spelling)) => spelling.as_str(),
            _ => "",
        };
        self.issue(
            "unsupported-enum-domain-mismatch",
            format!(
                "constant '{spelling}' is shared by several enum domains and appears \
                 where no parameter domain selects one"
            ),
            span,
        );
        true
    }
}
