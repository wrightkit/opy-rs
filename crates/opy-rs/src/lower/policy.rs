use crate::manifest::Function;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FunctionContext {
    ForIterable,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ContextualDomainOption {
    pub(crate) keyword: &'static str,
    pub(crate) domain: &'static str,
    pub(crate) target: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ContextualDomain {
    pub(crate) domain: &'static str,
    pub(crate) by: &'static str,
    pub(crate) options: &'static [ContextualDomainOption],
}

const CHASE_OPTIONS: &[ContextualDomainOption] = &[
    ContextualDomainOption {
        keyword: "rate",
        domain: "ChaseRateReeval",
        target: "chaseAtRate",
    },
    ContextualDomainOption {
        keyword: "duration",
        domain: "ChaseTimeReeval",
        target: "chaseOverTime",
    },
];

const CHASE: ContextualDomain = ContextualDomain {
    domain: "ChaseReeval",
    by: "rate",
    options: CHASE_OPTIONS,
};

pub(crate) fn contextual_domain(function_id: &str) -> Option<&'static ContextualDomain> {
    (function_id == "chase").then_some(&CHASE)
}

pub(crate) fn is_contextual_domain(domain: &str) -> bool {
    domain == CHASE.domain
}

pub(crate) fn function_context(function_id: &str) -> Option<FunctionContext> {
    (function_id == "range").then_some(FunctionContext::ForIterable)
}

/// A receiver constraint a member call enforces — typed OverPy policy keyed
/// on the member id. The manifest's `Function::receiver` category is
/// descriptive signature metadata and does not select these checks
/// (issue #458).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverRequirement {
    /// `.append` / `.remove`: the receiver must be assignable (the pinned
    /// reference's "Cannot modify or assign to …" rejection).
    Assignable,
    /// `.format`: the receiver must be a string literal.
    StringLiteral,
}

impl ReceiverRequirement {
    /// A human-readable description of the requirement for diagnostics.
    pub(crate) fn describe(self) -> &'static str {
        match self {
            ReceiverRequirement::Assignable => "an assignable variable",
            ReceiverRequirement::StringLiteral => "a string literal",
        }
    }
}

/// The receiver requirement a member enforces, if any. Members without an
/// entry accept any receiver: the pinned reference does not type-check
/// player-oriented receivers.
pub(crate) fn member_receiver_requirement(member_id: &str) -> Option<ReceiverRequirement> {
    match member_id {
        "append" | "remove" => Some(ReceiverRequirement::Assignable),
        "format" => Some(ReceiverRequirement::StringLiteral),
        _ => None,
    }
}

/// The parameter indices of `function_id` that must bind a variable
/// reference (a global or player variable): the chase family's first
/// argument selects the global/player emission form. Typed policy keyed on
/// the function id — the manifest's `Param::variable` flag records the same
/// fact descriptively and does not select this enforcement (issue #458).
pub(crate) fn variable_args(function_id: &str) -> &'static [usize] {
    match function_id {
        "chase" | "chaseAtRate" | "chaseOverTime" => &[0],
        _ => &[],
    }
}

pub(crate) fn validate(function: &Function) -> Result<(), String> {
    let Some(contextual) = contextual_domain(&function.id) else {
        return Ok(());
    };
    let by_param = function
        .params
        .iter()
        .find(|param| param.name == contextual.by)
        .ok_or_else(|| {
            format!(
                "function '{}' contextual domain '{}' references unknown selector parameter '{}'",
                function.id, contextual.domain, contextual.by
            )
        })?;
    if !function
        .params
        .iter()
        .any(|param| param.domain.as_deref() == Some(contextual.domain))
    {
        return Err(format!(
            "function '{}' contextual domain '{}' has no parameter declaring that domain",
            function.id, contextual.domain
        ));
    }
    let mut spellings = vec![by_param.name.as_str()];
    spellings.extend(by_param.alternate_names.iter().map(String::as_str));
    for option in contextual.options {
        if !spellings.contains(&option.keyword) {
            return Err(format!(
                "function '{}' contextual option '{}' is not a keyword spelling of selector parameter '{}'",
                function.id, option.keyword, by_param.name
            ));
        }
    }
    Ok(())
}
