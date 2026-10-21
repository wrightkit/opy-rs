use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Helpers {
    entries: BTreeMap<String, Vec<(String, String)>>,
}

impl Helpers {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub(crate) fn set_constant(&mut self, object: &str, key: &str, value: &str) {
        self.entries
            .entry(object.to_string())
            .or_default()
            .push((key.to_string(), value.to_string()));
    }

    pub(crate) fn entries(&self, object: &str) -> &[(String, String)] {
        self.entries.get(object).map_or(&[], Vec::as_slice)
    }
}
