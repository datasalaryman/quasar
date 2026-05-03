//! Diagnostic accumulator for IDL validation errors.

/// Severity level for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// A single diagnostic from the IDL pipeline.
#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub idl_path: Option<String>,
    pub hints: Vec<String>,
}

/// Accumulates diagnostics without early-return.
#[derive(Default)]
pub struct DiagnosticSink {
    diagnostics: Vec<Diagnostic>,
    has_errors: bool,
}

impl DiagnosticSink {
    pub fn error(&mut self, message: impl Into<String>) -> &mut Diagnostic {
        self.has_errors = true;
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            message: message.into(),
            idl_path: None,
            hints: Vec::new(),
        });
        self.diagnostics.last_mut().unwrap()
    }

    pub fn warning(&mut self, message: impl Into<String>) -> &mut Diagnostic {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            message: message.into(),
            idl_path: None,
            hints: Vec::new(),
        });
        self.diagnostics.last_mut().unwrap()
    }

    pub fn has_errors(&self) -> bool {
        self.has_errors
    }

    pub fn finish(self) -> Result<(), Vec<Diagnostic>> {
        if self.has_errors {
            Err(self.diagnostics)
        } else {
            Ok(())
        }
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

impl Diagnostic {
    pub fn at(mut self, path: impl Into<String>) -> Self {
        self.idl_path = Some(path.into());
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hints.push(hint.into());
        self
    }
}
