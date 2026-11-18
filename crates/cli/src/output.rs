//! Structured JSON / human output and standardized error reporting.

use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct GlobalOpts {
    pub lib: Option<PathBuf>,
    pub json: bool,
    pub dry_run: bool,
    pub quiet: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub severity: &'static str, // "error" | "warning" | "hint"
    pub rule: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loc: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix_hint: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CliError {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub valid_values: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<serde_json::Value>,
}

#[derive(Debug)]
pub struct CliFailureInner {
    pub exit_code: i32,
    pub error: CliError,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
pub struct CliFailure(pub Box<CliFailureInner>);

impl std::ops::Deref for CliFailure {
    type Target = CliFailureInner;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for CliFailure {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl std::fmt::Display for CliFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.error.code, self.error.message)
    }
}

impl std::error::Error for CliFailure {}

impl CliFailure {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self(Box::new(CliFailureInner {
            exit_code: 1,
            error: CliError { code, message: message.into(), valid_values: vec![], hint: None, example: None, candidates: vec![] },
            diagnostics: vec![],
        }))
    }
    pub fn usage(message: impl Into<String>) -> Self {
        let mut f = Self::new("INVALID_ARGS", message);
        f.exit_code = 2;
        f
    }
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.error.hint = Some(hint.into());
        self
    }
    pub fn with_example(mut self, ex: impl Into<String>) -> Self {
        self.error.example = Some(ex.into());
        self
    }
    pub fn with_valid(mut self, vals: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.error.valid_values = vals.into_iter().map(Into::into).collect();
        self
    }
    pub fn with_candidates(mut self, cands: Vec<serde_json::Value>) -> Self {
        self.error.candidates = cands;
        self
    }
    pub fn with_diagnostics(mut self, diags: Vec<Diagnostic>) -> Self {
        self.diagnostics = diags;
        self
    }
}

impl From<anyhow::Error> for CliFailure {
    fn from(e: anyhow::Error) -> Self {
        match e.downcast::<CliFailure>() {
            Ok(f) => f,
            Err(e) => CliFailure::new("IO_OR_CORE_ERROR", format!("{e:#}")),
        }
    }
}

fn slice_empty<T>(s: &&[T]) -> bool { s.is_empty() }

#[derive(Serialize)]
struct SuccessEnvelope<'a, T: Serialize> {
    ok: bool,
    command: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    library: Option<String>,
    dry_run: bool,
    data: T,
    #[serde(skip_serializing_if = "slice_empty")]
    warnings: &'a [String],
    #[serde(skip_serializing_if = "slice_empty")]
    diagnostics: &'a [Diagnostic],
}

#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    ok: bool,
    command: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    library: Option<String>,
    error: &'a CliError,
    #[serde(skip_serializing_if = "slice_empty")]
    diagnostics: &'a [Diagnostic],
}

pub fn print_success<T: Serialize>(
    opts: &GlobalOpts,
    command: &str,
    library: Option<&std::path::Path>,
    data: T,
    warnings: &[String],
    diagnostics: &[Diagnostic],
    human: &str,
) {
    if opts.quiet { return; }
    if opts.json {
        let env = SuccessEnvelope {
            ok: true,
            command,
            library: library.map(|p| p.display().to_string()),
            dry_run: opts.dry_run,
            data,
            warnings,
            diagnostics,
        };
        println!("{}", serde_json::to_string_pretty(&env).unwrap_or_else(|_| "{}".into()));
    } else {
        if !human.is_empty() { println!("{human}"); }
        for w in warnings { eprintln!("warning: {w}"); }
        for d in diagnostics {
            let loc = d.loc.as_deref().map(|l| format!(" [{l}]")).unwrap_or_default();
            let hint = d.fix_hint.as_deref().map(|h| format!(" (hint: {h})")).unwrap_or_default();
            eprintln!("{}:{} {}: {}{}", d.severity, loc, d.rule, d.message, hint);
        }
    }
}

pub fn print_error(opts: &GlobalOpts, command: &str, library: Option<&std::path::Path>, fail: &CliFailure) {
    if opts.json {
        let env = ErrorEnvelope {
            ok: false,
            command,
            library: library.map(|p| p.display().to_string()),
            error: &fail.error,
            diagnostics: &fail.diagnostics,
        };
        println!("{}", serde_json::to_string_pretty(&env).unwrap_or_else(|_| "{}".into()));
    } else {
        eprintln!("error [{}]: {}", fail.error.code, fail.error.message);
        if !fail.error.valid_values.is_empty() {
            eprintln!("valid values: {}", fail.error.valid_values.join(", "));
        }
        if !fail.error.candidates.is_empty() {
            for c in &fail.error.candidates { eprintln!("  candidate: {c}"); }
        }
        if let Some(h) = &fail.error.hint { eprintln!("hint: {h}"); }
        if let Some(ex) = &fail.error.example { eprintln!("example: {ex}"); }
        for d in &fail.diagnostics {
            let loc = d.loc.as_deref().map(|l| format!(" [{l}]")).unwrap_or_default();
            eprintln!("  {}{} {}: {}", d.severity, loc, d.rule, d.message);
        }
    }
}
