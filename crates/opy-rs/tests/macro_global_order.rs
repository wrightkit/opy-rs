use std::path::Path;

use opy_rs::tooling;

/// The pinned OverPy validates the rest of a declaration before it reports a
/// macro/global name conflict, so the invalid index is the first diagnostic.
#[test]
fn invalid_global_index_precedes_the_macro_conflict() {
    let checked = tooling::check(
        "macro shared = 5\nglobalvar shared nonsense\n",
        "main.opy",
        Path::new(""),
    );
    assert_eq!(checked.diagnostics[0].code, "parse-error");
}
