use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub file: u32,
    pub start: Position,
    pub end: Position,
    /// The macro invocation that macro expansion attributed this node to.
    ///
    /// The range above stays the definition site, which diagnostics and
    /// `wir` provenance read. The optimization state and the source map read
    /// the site, because upstream substitutes macro text there. A node
    /// with no definition span is attributed with a synthetic span whose
    /// range repeats the site and which [`Site::synthetic`] marks. Producers
    /// never set this field, and it is not part of the wire payload.
    #[serde(skip)]
    pub site: Option<Site>,
}

/// A macro invocation site recorded on a [`Span`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Site {
    pub file: u32,
    pub start: Position,
    pub end: Position,
    /// The node has no definition span: the span's range only repeats this
    /// site.
    pub synthetic: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub col: u32,
}

impl Span {
    pub(crate) fn synthetic(site: Site) -> Span {
        Span {
            file: site.file,
            start: site.start,
            end: site.end,
            site: Some(Site {
                synthetic: true,
                ..site
            }),
        }
    }

    pub(crate) fn is_synthetic(&self) -> bool {
        self.site.is_some_and(|site| site.synthetic)
    }

    pub(crate) fn definition(span: Option<Span>) -> Option<Span> {
        span.filter(|span| !span.is_synthetic())
            .map(|span| Span { site: None, ..span })
    }

    pub(crate) fn attributed(self) -> Span {
        let Site {
            file, start, end, ..
        } = self.site.unwrap_or(self.range());
        Span {
            file,
            start,
            end,
            site: None,
        }
    }

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
            synthetic: false,
        }
    }
}
