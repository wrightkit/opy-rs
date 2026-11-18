use super::*;

impl<'a> Emitter<'a> {
    /// The OPY call name of a Workshop action call, or `None` after reporting
    /// an action that lowers to a dedicated Workshop action. The
    /// player-variable chase actions share the call of their global
    /// counterparts: the variable argument is `player.variable`, and lowering
    /// picks the player form from it.
    pub(super) fn call_action_name<'n>(
        &mut self,
        name: &'n str,
        span: Option<Span>,
    ) -> Option<&'n str> {
        if DEDICATED_ACTION_NAMES.contains(&name) {
            self.issue(
                "unsupported-action-call",
                format!(
                    "action call '{name}' is lowered to a dedicated Workshop action \
                     and has no reconstructible call form"
                ),
                span,
            );
            return None;
        }
        Some(match name {
            "chasePlayerVariableAtRate" => "chaseAtRate",
            "chasePlayerVariableOverTime" => "chaseOverTime",
            "stopChasingPlayerVariable" => "stopChasingVariable",
            _ => name,
        })
    }
}
