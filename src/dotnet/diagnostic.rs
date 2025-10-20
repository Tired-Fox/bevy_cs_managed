use std::path::{Path, PathBuf};

#[derive(serde::Deserialize)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(serde::Deserialize)]
#[serde(rename_all="PascalCase")]
pub struct Diagnostic {
    pub filename: PathBuf,
    pub line: usize,
    pub column: usize,
    pub severity: Severity,
    pub code: String,
    pub message: String,
}

impl Diagnostic {
    pub fn log(&self) {
        match self.severity {
            Severity::Warning => log::warn!(
                "[msbuild {}] {} {},{}: {}",
                self.code,
                self.filename.file_name().unwrap().to_string_lossy(),
                self.line,
                self.column,
                self.message
            ),
            Severity::Error => log::error!(
                "[msbuild {}] {} {},{}: {}",
                self.code,
                self.filename.file_name().unwrap().to_string_lossy(),
                self.line,
                self.column,
                self.message
            ),
        }
    }

    #[allow(dead_code)]
    pub fn log_with_base(&self, base: &Path) {
        let base = dunce::canonicalize(base).unwrap();
        match self.severity {
            Severity::Warning => log::warn!(
                "[msbuild {}] {} {},{}: {}",
                self.code,
                self.filename.strip_prefix(base).unwrap_or(&self.filename).display(),
                self.line,
                self.column,
                self.message
            ),
            Severity::Error => log::error!(
                "[msbuild {}] {} {},{}: {}",
                self.code,
                self.filename.strip_prefix(base).unwrap_or(&self.filename).display(),
                self.line,
                self.column,
                self.message
            ),
        }
    }
}
