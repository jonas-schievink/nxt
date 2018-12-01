use codemap_diagnostic::{Diagnostic, Emitter};

use std::str::FromStr;
use std::ops::Index;
use std::ops::IndexMut;
use std::marker::PhantomData;

/// Trait for all types that have access to a diagnostic emitter.
///
/// Any type implementing this can be passed to `ResultExt::print_diagnostic`.
pub trait DiagnosticEmitter {
    fn emit_diagnostics(&mut self, diags: &[Diagnostic]);
}

impl<'a> DiagnosticEmitter for Emitter<'a> {
    fn emit_diagnostics(&mut self, diags: &[Diagnostic]) {
        self.emit(diags);
    }
}

/// If a structured diagnostic is returned as an error, it will be printed to
/// the console and replaced with this type to signal that no further error
/// printing is needed.
#[derive(Debug, Fail)]
#[fail(display = "(this should not be printed)")]
pub struct ErrorAlreadyPrinted;

pub trait ResultExt<T> {
    fn print_diagnostic<M>(self, emitter: &mut M) -> Result<T, ErrorAlreadyPrinted>
    where
        M: DiagnosticEmitter;
}

impl<T, E> ResultExt<T> for Result<T, E>
where
    E: Into<Diagnostic>,
{
    fn print_diagnostic<M>(self, emitter: &mut M) -> Result<T, ErrorAlreadyPrinted>
    where
        M: DiagnosticEmitter,
    {
        self.map_err(|e| {
            emitter.emit_diagnostics(&[e.into()]);
            ErrorAlreadyPrinted
        })
    }
}

#[derive(Debug, Copy, Clone)]
pub enum ColorConfig {
    Auto,
    Always,
    Never,
}

impl Default for ColorConfig {
    fn default() -> Self {
        ColorConfig::Auto
    }
}

impl FromStr for ColorConfig {
    type Err = InvalidColorConfig;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "auto" => ColorConfig::Auto,
            "always" => ColorConfig::Always,
            "never" => ColorConfig::Never,
            _ => return Err(InvalidColorConfig),
        })
    }
}

impl Into<::codemap_diagnostic::ColorConfig> for ColorConfig {
    fn into(self) -> ::codemap_diagnostic::ColorConfig {
        match self {
            ColorConfig::Auto => ::codemap_diagnostic::ColorConfig::Auto,
            ColorConfig::Always => ::codemap_diagnostic::ColorConfig::Always,
            ColorConfig::Never => ::codemap_diagnostic::ColorConfig::Never,
        }
    }
}

#[derive(Debug, Fail)]
#[fail(display = "invalid color configuration (try `always` or `never`)")]
pub struct InvalidColorConfig;

/// A `Vec<T>` that can only be indexed by `I`.
pub struct IndexVec<T, I>(Vec<T>, PhantomData<I>);

impl<T, I> IndexVec<T, I> where I: Into<usize> {
    pub fn new() -> Self {
        IndexVec(Vec::new(), PhantomData)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        IndexVec(Vec::with_capacity(capacity), PhantomData)
    }
}

impl<T, I> Index<I> for IndexVec<T, I> where I: Into<usize> {
    type Output = T;

    fn index(&self, index: I) -> &T {
        &self.0[index.into()]
    }
}

impl<T, I> IndexMut<I> for IndexVec<T, I> where I: Into<usize> {
    fn index_mut(&mut self, index: I) -> &mut T {
        &mut self.0[index.into()]
    }
}
