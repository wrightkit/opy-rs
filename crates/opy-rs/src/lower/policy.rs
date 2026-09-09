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
