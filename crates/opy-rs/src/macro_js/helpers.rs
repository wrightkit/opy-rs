//! Configurable helper globals exposed to JavaScript invocations.

use std::collections::BTreeMap;

/// Helper surface injected before every invocation.
///
/// The runtime always defines the upstream helper `vect` and the six constant
/// objects `Map`, `Hero`, `Gamemode`, `Color`, `Team`, `Button` (see
/// `builtInJsFunctions` in the OverPy reference `src/globalVars.ts`). The
/// constant entries are Workshop catalog data that `workshop-rs` owns, so this
/// crate ships them empty; populate them with [`Helpers::set_constant`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Helpers {
    entries: BTreeMap<String, Vec<(String, String)>>,
}

impl Helpers {
    /// Creates an empty helper set: only the builtin `vect` function and the
    /// six empty constant objects exist.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Adds one `key -> value` entry to the constant object `object`.
    ///
    /// Matches the upstream ABI where each entry is an UPPER_SNAKE key mapped
    /// to a string carrying the object prefix, e.g.
    /// `set_constant("Map", "KANEZAKA", "Map.KANEZAKA")` makes
    /// `Map.KANEZAKA === "Map.KANEZAKA"` inside scripts.
    #[cfg(test)]
    pub(crate) fn set_constant(&mut self, object: &str, key: &str, value: &str) {
        self.entries
            .entry(object.to_string())
            .or_default()
            .push((key.to_string(), value.to_string()));
    }

    /// Entries configured for `object`, in insertion order.
    pub(crate) fn entries(&self, object: &str) -> &[(String, String)] {
        self.entries.get(object).map_or(&[], Vec::as_slice)
    }
}
