/// Protocol versions this provider speaks, oldest first.
pub(crate) const PROTOCOL_VERSIONS: [&str; 8] =
    ["1.0", "1.1", "1.2", "1.3", "1.4", "1.5", "1.6", "1.7"];

pub(crate) fn version_at_least(protocol_version: &str, minimum: &str) -> bool {
    let rank = |version: &str| PROTOCOL_VERSIONS.iter().position(|known| *known == version);
    rank(protocol_version) >= rank(minimum)
}
