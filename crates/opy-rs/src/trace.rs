//! Phase timings of one request, for `opy-provider`'s trace file.
//!
//! Spans are recorded only while a recorder is installed on the current
//! thread. Without one, opening a span reads one thread-local value: it reads
//! no clock and allocates nothing.

use std::cell::RefCell;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

thread_local! {
    static RECORDER: RefCell<Option<Recorder>> = const { RefCell::new(None) };
}

/// One recorded span. `parent` is an index into the same list, and the list
/// is in order of first start.
#[derive(Debug, PartialEq, Eq)]
pub struct SpanRecord {
    pub parent: Option<usize>,
    pub name: &'static str,
    /// Offset of the (first) start from the recorder origin.
    pub start: Duration,
    pub duration: Duration,
    pub facts: Vec<(&'static str, u64)>,
}

struct Recorder {
    origin: Instant,
    spans: Vec<SpanRecord>,
    open: Vec<usize>,
}

/// Installs a recorder on this thread, replacing any previous one. Span
/// offsets count from `origin`.
pub fn start(origin: Instant) {
    RECORDER.with(|cell| {
        *cell.borrow_mut() = Some(Recorder {
            origin,
            spans: Vec::new(),
            open: Vec::new(),
        });
    });
}

/// Removes this thread's recorder and returns its spans.
pub fn finish() -> Option<Vec<SpanRecord>> {
    RECORDER
        .with(|cell| cell.borrow_mut().take())
        .map(|recorder| recorder.spans)
}

/// An open span; it closes when dropped.
#[must_use = "a span closes when it is dropped"]
pub struct Span(Option<(usize, Instant)>);

/// Opens a span nested in the innermost open span.
pub fn span(name: &'static str) -> Span {
    open(name, None)
}

/// Opens one occurrence of work that repeats inside a phase, such as a file
/// read. All occurrences under the same parent share one span: it starts at
/// the first occurrence, its duration is the sum of the occurrences, and its
/// `count_fact` counts them.
pub fn accumulate(name: &'static str, count_fact: &'static str) -> Span {
    open(name, Some(count_fact))
}

/// [`std::fs::read_to_string`] recorded as a `source.read` occurrence with
/// `files` and `bytes` facts.
pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    let span = accumulate("source.read", "files");
    let text = std::fs::read_to_string(path)?;
    span.add("bytes", text.len() as u64);
    Ok(text)
}

fn open(name: &'static str, count_fact: Option<&'static str>) -> Span {
    RECORDER.with(|cell| {
        let mut recorder = cell.borrow_mut();
        let Some(recorder) = recorder.as_mut() else {
            return Span(None);
        };
        let now = Instant::now();
        let parent = recorder.open.last().copied();
        let shared = count_fact.and_then(|_| {
            recorder
                .spans
                .iter()
                .position(|span| span.parent == parent && span.name == name)
        });
        let index = shared.unwrap_or_else(|| {
            recorder.spans.push(SpanRecord {
                parent,
                name,
                start: now.saturating_duration_since(recorder.origin),
                duration: Duration::ZERO,
                facts: Vec::new(),
            });
            recorder.spans.len() - 1
        });
        if let Some(fact) = count_fact {
            add_fact(&mut recorder.spans[index], fact, 1);
        }
        recorder.open.push(index);
        Span(Some((index, now)))
    })
}

fn add_fact(span: &mut SpanRecord, name: &'static str, value: u64) {
    match span.facts.iter_mut().find(|(fact, _)| *fact == name) {
        Some((_, total)) => *total += value,
        None => span.facts.push((name, value)),
    }
}

impl Span {
    /// Adds `value` to the span's numeric fact `name`.
    pub fn add(&self, name: &'static str, value: u64) {
        let Some((index, _)) = self.0 else {
            return;
        };
        let _ = RECORDER.try_with(|cell| {
            if let Some(span) = cell
                .borrow_mut()
                .as_mut()
                .and_then(|recorder| recorder.spans.get_mut(index))
            {
                add_fact(span, name, value);
            }
        });
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        let Some((index, started)) = self.0 else {
            return;
        };
        let elapsed = started.elapsed();
        let _ = RECORDER.try_with(|cell| {
            let mut recorder = cell.borrow_mut();
            let Some(recorder) = recorder.as_mut() else {
                return;
            };
            if let Some(span) = recorder.spans.get_mut(index) {
                span.duration += elapsed;
            }
            if recorder.open.last() == Some(&index) {
                recorder.open.pop();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::{accumulate, finish, span, start};

    #[test]
    fn records_nothing_without_a_recorder() {
        let outer = span("outer");
        assert!(outer.0.is_none());
        drop(outer);
        assert_eq!(finish(), None);
    }

    #[test]
    fn nests_spans_and_merges_repeated_work_under_one_parent() {
        start(Instant::now());
        {
            let _outer = span("outer");
            for bytes in [3, 4] {
                let read = accumulate("source.read", "files");
                read.add("bytes", bytes);
            }
            let _inner = span("inner");
            let _read = accumulate("source.read", "files");
        }
        drop(span("after"));
        let spans = finish().expect("recorder installed");
        let shape: Vec<_> = spans
            .iter()
            .map(|span| (span.parent, span.name, span.facts.clone()))
            .collect();
        assert_eq!(
            shape,
            [
                (None, "outer", vec![]),
                (Some(0), "source.read", vec![("files", 2), ("bytes", 7)]),
                (Some(0), "inner", vec![]),
                (Some(2), "source.read", vec![("files", 1)]),
                (None, "after", vec![]),
            ]
        );
        assert!(spans[1].duration <= spans[0].duration);
        assert!(spans[4].start >= spans[0].start + spans[0].duration);
    }
}
