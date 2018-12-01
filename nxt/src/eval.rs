use ast::*;
use config::Config;
use utils::ResultExt;
use value::Value;
use {parser, profile};

use codemap::{CodeMap, File};
use codemap_diagnostic::{Diagnostic, Emitter};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{fs, io};

/// Nix expression source (file, command line, ...).
pub enum Source<'a> {
    /// Read a Nix expression from a file.
    File {
        /// Path to the file to read.
        ///
        /// This specifies both the name of the source, the content, and the
        /// search path.
        path: &'a Path,
    },

    /// Read a Nix expression from a non-file source.
    Other {
        /// The Nix expression source code.
        source: &'a str,
        /// Name of this source.
        name: &'a str,
        /// The search path relative to which file references inside the source
        /// code are resolved.
        search_path: &'a Path,
    },
}

pub struct EvalContext<'a> {
    arenas: &'a Arenas<'a>,
    codemap: CodeMap,
    config: Config,
}

impl<'a> EvalContext<'a> {
    pub fn new(config: Config, arenas: &'a Arenas<'a>) -> Self {
        Self {
            arenas,
            codemap: CodeMap::new(),
            config,
        }
    }

    fn assimilate_source(&mut self, source: Source) -> Result<(Arc<File>, PathBuf), Error> {
        let (source, name, search_path) = match source {
            Source::File { path } => {
                let name = path.display().to_string();
                let mut search_path = path.to_path_buf();
                search_path.pop();

                let source = profile::profile("reading", path, || fs::read_to_string(path))?;
                (source, name, search_path)
            }
            Source::Other {
                source,
                name,
                search_path,
            } => (source.to_string(), name.to_string(), search_path.to_owned()),
        };

        let file = self.codemap.add_file(name, source);
        Ok((file, search_path))
    }

    /// Evaluates a Nix expression.
    ///
    /// This will parse the source code in `source` and then perform all
    /// necessary operations to return a `Value` corresponding to the top-level
    /// expression in the source.
    ///
    /// This process might read and parse more `.nix` files from the file
    /// system.
    pub fn eval(&mut self, source: Source) -> Result<Value<'a>, Error> {
        let (file, search_path) = self.assimilate_source(source)?;
        let raw_ast = parser::parse(&file).print_diagnostic(self)?;
        let ast = Ast::build(&self.arenas, file, &search_path, raw_ast).print_diagnostic(self)?;
        debug!("AST={:#?}", ast);

        self.eval_expr(ast.root())
    }

    fn eval_expr<'e>(&mut self, expr: &'e Expr<'e>) -> Result<Value<'e>, Error> {
        match expr {
            Expr::Value(val) => Ok((*val).clone()),
            _ => unimplemented!(),
        }
    }
}

impl<'a> ::utils::DiagnosticEmitter for EvalContext<'a> {
    fn emit_diagnostics(&mut self, diags: &[Diagnostic]) {
        let mut emitter = Emitter::stderr(self.config.color.into(), Some(&self.codemap));
        emitter.emit(diags);
    }
}

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "i/o error: {}", _0)]
    Io(#[fail(cause)] io::Error),

    #[fail(display = "(this should not be printed)")]
    AlreadyPrinted,
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::Io(e)
    }
}

impl From<::utils::ErrorAlreadyPrinted> for Error {
    fn from(_: ::utils::ErrorAlreadyPrinted) -> Self {
        Error::AlreadyPrinted
    }
}
