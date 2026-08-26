//! CLI-local presentation policy for human and GitHub Actions output.
//!
//! `opy-rs` owns structured diagnostics. This module owns only their
//! terminal, plain, and GitHub Actions presentation. Machine-readable output
//! is rendered by the command handlers before this boundary is entered.

use std::io::{IsTerminal, Write};

use crate::cli::{ColorArg, RendererArg};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DiagnosticSeverity {
    Error,
}

impl DiagnosticSeverity {
    fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PositionView {
    pub(crate) line: u32,
    pub(crate) col: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SpanView {
    pub(crate) path: String,
    pub(crate) start: PositionView,
    pub(crate) end: PositionView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiagnosticView {
    pub(crate) severity: DiagnosticSeverity,
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) span: Option<SpanView>,
}

pub(crate) struct CheckView {
    pub(crate) clean: bool,
    pub(crate) diagnostics: Vec<DiagnosticView>,
    pub(crate) file_count: usize,
    pub(crate) declaration_count: usize,
    pub(crate) rule_count: usize,
    pub(crate) symbol_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Presentation {
    renderer: Renderer,
    color: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Renderer {
    Terminal,
    Plain,
    GithubActions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuntimeEnvironment {
    github_actions: bool,
    ci: bool,
    stdout_terminal: bool,
    no_color: bool,
}

impl RuntimeEnvironment {
    fn process() -> Self {
        Self {
            github_actions: env_truthy("GITHUB_ACTIONS"),
            ci: env_truthy("CI"),
            stdout_terminal: std::io::stdout().is_terminal(),
            no_color: std::env::var_os("NO_COLOR").is_some(),
        }
    }
}

impl Presentation {
    pub(crate) fn from_cli(renderer: RendererArg, color: ColorArg) -> Self {
        Self::resolve(renderer, color, RuntimeEnvironment::process())
    }

    fn resolve(renderer: RendererArg, color: ColorArg, environment: RuntimeEnvironment) -> Self {
        let renderer = match renderer {
            RendererArg::Terminal => Renderer::Terminal,
            RendererArg::Plain => Renderer::Plain,
            RendererArg::GithubActions => Renderer::GithubActions,
            RendererArg::Auto => {
                if environment.github_actions {
                    Renderer::GithubActions
                } else if environment.ci || !environment.stdout_terminal {
                    Renderer::Plain
                } else {
                    Renderer::Terminal
                }
            }
        };
        let color = match color {
            ColorArg::Always => renderer == Renderer::Terminal,
            ColorArg::Never => false,
            ColorArg::Auto => renderer == Renderer::Terminal && !environment.no_color,
        };
        Self { renderer, color }
    }

    pub(crate) fn render_check(&self, view: &CheckView) {
        match self.renderer {
            Renderer::GithubActions => self.render_github_check(view),
            Renderer::Terminal | Renderer::Plain => {
                for diagnostic in &view.diagnostics {
                    eprintln!("{}", format_diagnostic(diagnostic, self.color));
                }
                if view.clean {
                    let summary = format!(
                        "check passed: {} file(s), {} declaration(s), {} rule entry(ies), {} symbol(s)",
                        view.file_count, view.declaration_count, view.rule_count, view.symbol_count,
                    );
                    if self.color {
                        println!("\x1b[32m{summary}\x1b[0m");
                    } else {
                        println!("{summary}");
                    }
                }
            }
        }
    }

    pub(crate) fn render_diagnostics(&self, command: &str, diagnostics: &[DiagnosticView]) {
        match self.renderer {
            Renderer::GithubActions => {
                for diagnostic in diagnostics {
                    emit_diagnostic_annotation(diagnostic);
                }
                eprintln!(
                    "::group::{}",
                    escape_workflow_data(&format!("opy-cli {command}"))
                );
                eprintln!("ERROR {command} ({} diagnostic(s))", diagnostics.len());
                eprintln!("::endgroup::");
                emit_summary(command, "ERROR");
            }
            Renderer::Terminal | Renderer::Plain => {
                for diagnostic in diagnostics {
                    eprintln!("{}", format_diagnostic(diagnostic, self.color));
                }
            }
        }
    }

    fn render_github_check(&self, view: &CheckView) {
        for diagnostic in &view.diagnostics {
            emit_diagnostic_annotation(diagnostic);
        }
        eprintln!("::group::opy-cli check");
        if view.clean {
            eprintln!("PASS check");
            eprintln!("::endgroup::");
            emit_summary("check", "PASS");
        } else {
            eprintln!("ERROR check ({} diagnostic(s))", view.diagnostics.len());
            eprintln!("::endgroup::");
            emit_summary("check", "ERROR");
        }
    }
}

fn env_truthy(name: &str) -> bool {
    match std::env::var(name) {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "no"
        ),
        Err(_) => false,
    }
}

fn format_diagnostic(diagnostic: &DiagnosticView, color: bool) -> String {
    let location = diagnostic.span.as_ref().map(|span| {
        format!(
            "\n  --> {}:{}:{}",
            span.path, span.start.line, span.start.col
        )
    });
    let message = if color {
        format!("\x1b[31m{}\x1b[0m", diagnostic.message)
    } else {
        diagnostic.message.clone()
    };
    format!(
        "{}[{}]: {}{}",
        diagnostic.severity.as_str(),
        diagnostic.code,
        message,
        location.unwrap_or_default()
    )
}

fn emit_diagnostic_annotation(diagnostic: &DiagnosticView) {
    let kind = diagnostic.severity.as_str();
    let mut properties = vec![format!(
        "title={}",
        escape_workflow_property(&diagnostic.code)
    )];
    if let Some(span) = diagnostic
        .span
        .as_ref()
        .filter(|span| is_real_source_path(&span.path))
    {
        properties.insert(0, format!("file={}", escape_workflow_property(&span.path)));
        properties.push(format!("line={}", span.start.line));
        properties.push(format!("col={}", span.start.col));
        properties.push(format!("endLine={}", span.end.line));
        properties.push(format!("endColumn={}", span.end.col));
    }
    eprintln!(
        "::{kind} {}::{}",
        properties.join(","),
        escape_workflow_data(&diagnostic.message)
    );
}

fn emit_summary(command: &str, status: &str) {
    let line = format!("opy-cli `{command}`: **{status}**");
    if let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        let result = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| writeln!(file, "{line}"));
        if let Err(error) = result {
            eprintln!(
                "::warning title=opy-cli summary::{}",
                escape_workflow_data(&error.to_string())
            );
        }
    } else {
        eprintln!(
            "::notice title=opy-cli summary::{}",
            escape_workflow_data(&line)
        );
    }
}

/// Workflow command properties additionally escape `:` and `,`.
pub(crate) fn escape_workflow_property(value: &str) -> String {
    escape_workflow_data(value)
        .replace(':', "%3A")
        .replace(',', "%2C")
}

/// Workflow command data escapes the GitHub Actions command delimiters.
pub(crate) fn escape_workflow_data(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn is_real_source_path(path: &str) -> bool {
    !path.is_empty() && !path.starts_with('<')
}

#[cfg(test)]
mod tests {
    use super::{RuntimeEnvironment, escape_workflow_data, escape_workflow_property};
    use crate::cli::{ColorArg, RendererArg};

    #[test]
    fn workflow_values_are_escaped_at_the_correct_boundary() {
        assert_eq!(escape_workflow_property("a:b,c%\n"), "a%3Ab%2Cc%25%0A");
        assert_eq!(escape_workflow_data("a:b,c%\n"), "a:b,c%25%0A");
    }

    #[test]
    fn explicit_renderer_and_color_override_environment_selection() {
        let environment = RuntimeEnvironment {
            github_actions: true,
            ci: true,
            stdout_terminal: false,
            no_color: true,
        };
        let github =
            super::Presentation::resolve(RendererArg::GithubActions, ColorArg::Always, environment);
        assert_eq!(github.renderer, super::Renderer::GithubActions);
        assert!(!github.color);

        let terminal =
            super::Presentation::resolve(RendererArg::Terminal, ColorArg::Always, environment);
        assert_eq!(terminal.renderer, super::Renderer::Terminal);
        assert!(terminal.color);

        let plain = super::Presentation::resolve(RendererArg::Auto, ColorArg::Auto, environment);
        assert_eq!(plain.renderer, super::Renderer::GithubActions);
        assert!(!plain.color);
    }
}
