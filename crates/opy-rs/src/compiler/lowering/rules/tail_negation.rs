use super::*;

pub(super) fn operand<'expr>(
    lowering: &mut Lowering<'_>,
    condition: &'expr Expr,
) -> Result<Option<&'expr Expr>, IntegrationError> {
    let Expr::Unary { op, operand, .. } = condition else {
        return Ok(None);
    };
    if op != "not" {
        return Ok(None);
    }
    let condition = lowering.lower_value(condition)?;
    Ok(matches!(
        lowering.materialize_value(condition),
        workshop_rs::Value::Call { name, .. } if name == "not"
    )
    .then_some(operand.as_ref()))
}
