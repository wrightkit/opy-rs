use std::path::Path;

#[test]
fn horizontal_facing_angle_arity_failure_keeps_source_provenance() {
    let source = "rule \"r\":\n    @Event eachPlayer\n    @Condition eventPlayer.getHorizontalFacingAngle(1) == 0\n";
    let error = crate::compile(source, "source.opy", Path::new("."))
        .expect_err("member arity must be rejected");
    assert_eq!(error.code, "invalid-arity");
    assert_eq!(
        error
            .span
            .expect("diagnostic is source-attributed")
            .start
            .line,
        3
    );
}
