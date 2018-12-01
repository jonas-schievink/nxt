//! Defines dynamically typed Nix expression values.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use tendril::StrTendril;

type Expr = rnix::parser::Node;

#[derive(Debug, Copy, Clone)]
pub enum Type {
    String,
    Int,
    Float,
    Path,
    Bool,
    Null,
    List,
    Set,
}

/// The value a Nix expression was evaluated to.
///
/// A Nix expression will remain in "unevaluated" state until its value is
/// needed, since Nix is a lazily evaluated language. Such unevaluated
/// `Expr`s might be referred to by already evaluated `Value`s.
#[derive(Debug, Clone)]
pub enum Value {
    /// A string or URI.
    ///
    /// The unquoted URI notation just results in a string, there is no separate
    /// URI type.
    String(StrTendril),

    /// A signed integer.
    ///
    /// Range appears to be exactly an `i64`, except that the Nix parser rejects
    /// `-9223372036854775808`, the smallest `i64`, as invalid. However,
    /// `-9223372036854775807 - 1` still results in `-9223372036854775808`, so
    /// this is probably a bug in its parser.
    Int(i64),

    Float(f64),

    /// A (home-)relative or absolute path, expanded to an absolute path when
    /// parsing.
    ///
    /// Paths like `<this>`, which are searched for in `NIX_PATH`, are only
    /// searched for when they're needed (they're `Expr`essions, not `Value`s).
    /// When they're not found, evaluation aborts, while normal `Path`s can
    /// refer to files that don't exist.
    Path(PathBuf),

    Bool(bool),

    Null, // TODO null tracking

    List(Vec<Expr>),

    Set(BTreeMap<String, Expr>),
}

impl Value {
    pub fn type_(&self) -> Type {
        match self {
            Value::String(_) => Type::String,
            Value::Int(_) => Type::Int,
            Value::Float(_) => Type::Float,
            Value::Path(_) => Type::Path,
            Value::Bool(_) => Type::Bool,
            Value::Null => Type::Null,
            Value::List(_) => Type::List,
            Value::Set(_) => Type::Set,
        }
    }

    /// If this value is a boolean, returns it. If not, returns `None`.
    pub fn as_bool(&self) -> Option<bool> {
        if let Value::Bool(b) = self {
            Some(*b)
        } else {
            None
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Value::String(s) => write!(f, "\"{}\"", s),
            Value::Int(i) => i.fmt(f),
            Value::Float(flt) => flt.fmt(f),
            Value::Path(p) => p.display().fmt(f),
            Value::Bool(b) => b.fmt(f),
            Value::Null => f.write_str("null"),
            Value::List(vec) => f.debug_list().entries(vec.iter()).finish(),
            Value::Set(map) => f.debug_map().entries(map.iter()).finish(),
        }
    }
}
