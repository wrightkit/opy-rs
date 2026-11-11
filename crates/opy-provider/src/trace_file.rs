//! Phase timings for Wright's `--trace`, appended to the file named by
//! `WRIGHT_PROVIDER_TRACE_FILE` as one `wright-provider-trace/v1` line per
//! request.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use opy_rs::trace::SpanRecord;
use serde_json::{Map, Value, json};

const ENV: &str = "WRIGHT_PROVIDER_TRACE_FILE";
const SCHEMA: &str = "wright-provider-trace/v1";
const METHODS: [&str; 12] = [
    "lpp/initialize",
    "lpp/shutdown",
    "lpp/check",
    "lpp/compile",
    "lpp/reconstruct",
    "lpp/symbols",
    "lpp/definition",
    "lpp/references",
    "lpp/rename",
    "lpp/validateEdits",
    "lpp/lookup",
    "lpp/resolveIds",
];

/// The trace file, read once per process: the client sets the variable when
/// it spawns the provider, and the provider's environment does not change
/// afterwards. A relative path disables tracing.
pub(crate) fn path_from_env() -> Option<PathBuf> {
    std::env::var_os(ENV)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// The span that covers the handler of `method`: `compile` for `lpp/compile`.
/// A method the provider does not serve gets no handler span.
pub(crate) fn handler_span(method: &str) -> Option<opy_rs::trace::Span> {
    METHODS
        .iter()
        .find(|known| **known == method)
        .and_then(|known| known.strip_prefix("lpp/"))
        .map(opy_rs::trace::span)
}

/// Ends this request's recording and appends its line to `path`. A write
/// failure is ignored: tracing never fails a request.
pub(crate) fn append(path: &Path) {
    let Some(spans) = opy_rs::trace::finish() else {
        return;
    };
    let mut line = line(&spans).to_string();
    line.push('\n');
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(line.as_bytes()));
}

fn line(spans: &[SpanRecord]) -> Value {
    let method = spans
        .iter()
        .filter(|span| span.parent.is_none())
        .find_map(|span| {
            METHODS
                .iter()
                .find(|known| known.strip_prefix("lpp/") == Some(span.name))
        });
    let spans: Vec<Value> = spans
        .iter()
        .enumerate()
        .map(|(id, span)| {
            let facts: Map<String, Value> = span
                .facts
                .iter()
                .map(|(name, value)| ((*name).to_owned(), json!(value)))
                .collect();
            json!({
                "id": id,
                "parent": span.parent,
                "name": format!("provider.{}", span.name),
                "startNs": nanos(span.start),
                "durationNs": nanos(span.duration),
                "facts": facts,
            })
        })
        .collect();
    json!({ "schema": SCHEMA, "method": method, "spans": spans })
}

fn nanos(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}
