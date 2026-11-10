//! Encodes the `workshop-rs/mapped-text-v1` content of an LPP artifact.
//!
//! The content has object keys in sorted order at every depth, the order a
//! `serde_json::Value` round trip produces. This module streams the document
//! once into that order, without building a `Value` tree, and rewrites each
//! `files[].path` to the URI supplied by the caller.

use std::fmt;
use std::ops::Range;

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserializer, Serialize};

#[derive(Clone, Copy)]
enum Role {
    Document,
    FileTable,
    File,
    FilePath,
    Plain,
}

struct Node<'a, 'out> {
    role: Role,
    uri_for: &'a dyn Fn(&str) -> String,
    output: &'out mut Vec<u8>,
}

/// Returns `json` with sorted object keys and every `files[].path` replaced by
/// `uri_for(path)`.
pub(crate) fn encode(
    json: &str,
    uri_for: &dyn Fn(&str) -> String,
) -> Result<String, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    let mut output = Vec::with_capacity(json.len());
    Node {
        role: Role::Document,
        uri_for,
        output: &mut output,
    }
    .deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(String::from_utf8(output).expect("serde_json writes UTF-8"))
}

fn write<T: Serialize + ?Sized, E: de::Error>(output: &mut Vec<u8>, value: &T) -> Result<(), E> {
    serde_json::to_writer(output, value).map_err(E::custom)
}

impl<'de> DeserializeSeed<'de> for Node<'_, '_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Node<'_, '_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<(), E> {
        write(self.output, &value)
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<(), E> {
        write(self.output, &value)
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<(), E> {
        write(self.output, &value)
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<(), E> {
        write(self.output, &value)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<(), E> {
        match self.role {
            Role::FilePath => write(self.output, &(self.uri_for)(value)),
            _ => write(self.output, value),
        }
    }

    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        self.output.extend_from_slice(b"null");
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let role = match self.role {
            Role::FileTable => Role::File,
            _ => Role::Plain,
        };
        self.output.push(b'[');
        let mut count = 0;
        loop {
            let mark = self.output.len();
            if count > 0 {
                self.output.push(b',');
            }
            let item = Node {
                role,
                uri_for: self.uri_for,
                output: &mut *self.output,
            };
            match seq.next_element_seed(item)? {
                Some(()) => count += 1,
                None => {
                    self.output.truncate(mark);
                    break;
                }
            }
        }
        self.output.push(b']');
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let start = self.output.len();
        let mut entries: Vec<(String, Range<usize>)> = Vec::new();
        while let Some(key) = map.next_key::<String>()? {
            let role = match (self.role, key.as_str()) {
                (Role::Document, "files") => Role::FileTable,
                (Role::File, "path") => Role::FilePath,
                _ => Role::Plain,
            };
            let value_start = self.output.len();
            let value = Node {
                role,
                uri_for: self.uri_for,
                output: &mut *self.output,
            };
            map.next_value_seed(value)?;
            entries.push((key, value_start..self.output.len()));
        }
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        let body = self.output.split_off(start);
        self.output.push(b'{');
        for (index, (key, range)) in entries.iter().enumerate() {
            if index > 0 {
                self.output.push(b',');
            }
            write::<str, A::Error>(self.output, key)?;
            self.output.push(b':');
            self.output
                .extend_from_slice(&body[range.start - start..range.end - start]);
        }
        self.output.push(b'}');
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::encode;

    fn with_prefix(path: &str) -> String {
        format!("file:///project/{path}")
    }

    /// The `Value` round trip this module replaces, kept as the test oracle.
    fn value_round_trip(json: &str) -> String {
        let mut content: Value = serde_json::from_str(json).expect("json");
        for file in content["files"].as_array_mut().expect("files") {
            let uri = with_prefix(file["path"].as_str().expect("path"));
            file["path"] = Value::String(uri);
        }
        content.to_string()
    }

    const DOCUMENT: &str = r#"{"format":"workshop-rs/mapped-text-v1","text":"rule \"a\u0001é\":\n\tdefault","files":[{"path":"src/main.opy"},{"path":"src/ünï.opy"}],"shape":{"global_variables":0,"player_variables":2,"subroutines":1,"rules":[{"conditions":1,"actions":2}]},"spans":[{"file":1,"start":{"line":3,"column":2},"end":{"line":3,"column":9},"children":[{"identifier_span":{"file":0,"start":{"line":1,"column":1},"end":{"line":1,"column":4}}}]},{"rule":0}]}"#;

    #[test]
    fn sorts_keys_at_every_depth_in_the_value_round_trip_order() {
        let encoded = encode(DOCUMENT, &with_prefix).expect("encodes");
        assert_eq!(
            encoded,
            r#"{"files":[{"path":"file:///project/src/main.opy"},{"path":"file:///project/src/ünï.opy"}],"format":"workshop-rs/mapped-text-v1","shape":{"global_variables":0,"player_variables":2,"rules":[{"actions":2,"conditions":1}],"subroutines":1},"spans":[{"children":[{"identifier_span":{"end":{"column":4,"line":1},"file":0,"start":{"column":1,"line":1}}}],"end":{"column":9,"line":3},"file":1,"start":{"column":2,"line":3}},{"rule":0}],"text":"rule \"a\u0001é\":\n\tdefault"}"#
        );
    }

    #[test]
    fn matches_the_value_round_trip_byte_for_byte() {
        assert_eq!(
            encode(DOCUMENT, &with_prefix).expect("encodes"),
            value_round_trip(DOCUMENT)
        );
    }
}
