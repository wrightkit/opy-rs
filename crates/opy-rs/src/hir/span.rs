//! Source spans of HIR nodes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub file: u32,
    pub start: Position,
    pub end: Position,
    /// The macro invocation that macro expansion attributed this node to.
    ///
    /// The range above stays the definition site: lowering decisions and
    /// diagnostics read it, and only the source map reads the site. A node
    /// with no definition span is attributed with a synthetic span whose
    /// range repeats the site (see [`Span::is_synthetic`]). Producers never
    /// set this field, and it is not part of the wire payload.
    #[serde(skip)]
    pub site: Option<Site>,
}

/// A macro invocation site recorded on a [`Span`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Site {
    pub file: u32,
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub col: u32,
}

impl Span {
    /// The span of a node that has no definition span, attributed to `site`.
    pub(crate) fn synthetic(site: Site) -> Span {
        Span {
            file: site.file,
            start: site.start,
            end: site.end,
            site: Some(site),
        }
    }

    /// Whether the range only repeats the site: the node has no definition
    /// span. A real definition range never equals its site, because an
    /// invocation site lies outside every macro body.
    pub(crate) fn is_synthetic(&self) -> bool {
        self.site == Some(self.range())
    }

    /// The definition-site view of `span`: what the span is without macro
    /// attribution.
    pub(crate) fn definition(span: Option<Span>) -> Option<Span> {
        span.filter(|span| !span.is_synthetic())
            .map(|span| Span { site: None, ..span })
    }

    /// The attributed view: the site when one is recorded, else the range.
    pub(crate) fn attributed(self) -> Span {
        let Site { file, start, end } = self.site.unwrap_or(self.range());
        Span {
            file,
            start,
            end,
            site: None,
        }
    }

    /// Attribute `span` to the invocation span `site`, keeping its definition
    /// range. A span-less or synthetic span becomes the synthetic span of
    /// `site`.
    pub(crate) fn attribute(span: &mut Option<Span>, site: Span) {
        let site = site.range();
        match span {
            Some(span) if !span.is_synthetic() => span.site = Some(site),
            _ => *span = Some(Span::synthetic(site)),
        }
    }

    fn range(&self) -> Site {
        Site {
            file: self.file,
            start: self.start,
            end: self.end,
        }
    }
}
