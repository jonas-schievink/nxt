//! Defines dynamically typed Nix expression values.

use std::path::PathBuf;
use std::collections::BTreeMap;

type Expr = rnix::parser::Node;

/// The value a Nix expression was evaluated to.
///
/// A Nix expression will remain in "unevaluated" state until its value is
/// needed, since Nix is a lazily evaluated language. Such unevaluated
/// `Expr`s might be referred to by already evaluated `Value`s.
#[derive(Debug)]
pub enum Value {
    /// A string or URI.
    ///
    /// The unquoted URI notation just results in a string, there is no separate
    /// URI type.
    String(String),

    /// A signed integer.
    ///
    /// Range appears to be exactly an `i64`, except that the Nix parser rejects
    /// `-9223372036854775808`, the smallest `i64`, as invalid. However,
    /// `-9223372036854775807 - 1` still results in `-9223372036854775808`, so
    /// this is probably a bug in its parser.
    Int(i64),

    Float(f64),

    Path(PathBuf),

    Bool(bool),

    Null,   // TODO null tracking

    List(Vec<Expr>),

    Set(BTreeMap<String, Expr>),
}
