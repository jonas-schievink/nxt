//! A custom AST that simplifies working with it.
//!
//! The `rnix` parser has its own AST based on `rowan`, which requires lots of
//! dynamic type conversions and indirection through generic parameters. It also
//! stores metadata like spans inside the nodes.
//!
//! This AST is more abstract (it "desugars" some constructs into simpler ones)
//! and focuses on the actual semantics, storing metadata like spans outside the
//! main tree. It is used directly to evaluate Nix expressions, which it tries
//! to make as simple and efficient as possible.

mod build;

use self::build::Builder;
use parser::{self, Error};
use value::Value;

use codemap::{File, Span};
use rnix::parser::Types;
use rowan::TreeRoot;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use toolshed::Arena as CopyArena;
use typed_arena::Arena as TypedArena;

/*

AST building:

* Arena-backed
* Fully visit every node that introduces a new variable scope, and resolve all
  idents. Any mention of an undeclared variable is actually an error in Nix, so
  it must be an error here too.

*/

/// An expression.
#[derive(Debug, Copy, Clone)]
pub enum Expr<'a> {
    /// `<lambda> <argument>`
    ///
    /// Calls a lambda with an argument.
    Apply {
        lambda: &'a Expr<'a>,
        argument: &'a Expr<'a>,
    },

    /// `assert <assertion>; <then>`
    ///
    /// Evaluates `assertion`, which must be a boolean.
    ///
    /// If the value is `true`, evaluates to `then`. Else, an error is reported
    /// and evaluation aborts.
    Assert {
        assertion: &'a Expr<'a>,
        then: &'a Expr<'a>,
    },

    /// `if <cond> then <then> else <els>`
    ///
    /// Evaluates `cond`, which must be a boolean.
    ///
    /// Then evaluates to `then` if `cond` is `true`, or to `els` otherwise.
    IfElse {
        cond: &'a Expr<'a>,
        then: &'a Expr<'a>,
        els: &'a Expr<'a>,
    },

    /// `set.index`
    ///
    /// Evaluates `index`, then evaluates to the corresponding member of `set`.
    IndexSet {
        set: &'a Expr<'a>,
        index: &'a Expr<'a>,
    },

    /// Instantiate a lambda.
    Lambda {
        argument: (), // complex due to pattern matching
        body: &'a Expr<'a>,
    },

    /// A `<path>` expression.
    ///
    /// Note that this is only for angle-bracketed paths that are searched for
    /// in `NIX_PATH`, not for other kinds of paths, which are just `Value`s.
    NixPath(&'a Path),

    /// A literal value.
    Value(&'a Value),

    /// A local variable.
    Variable(Variable),
}

/// A resolved local variable.
///
/// At runtime, every local variable is associated to an `&'a Expr<'a>`.
/// It's also possible to cache the `Value` this expression resolves to.
#[derive(Debug, Copy, Clone)]
pub struct Variable(u32);

impl Into<usize> for Variable {
    fn into(self) -> usize { self.0 as usize }
}

/// Information about a local variable.
#[derive(Debug, Copy, Clone)]
pub struct VarInfo<'a> {
    /// The span containing the variable name at the declaration site.
    pub decl_span: Span,
    /// The variable name (can collide with other variables).
    pub name: &'a str,
    /// The expression assigned to the variable.
    pub expr: &'a Expr<'a>,
}

/// An attribute or variable path.
///
/// `a`, `"a"."a"`, `x.y`, `x."${interpolated} string"`.
///
/// An attribute making use of `"${interpolation}"` is also called "dynamic
/// attribute" and is not allowed in `let .. in ..` bindings.
///
/// This is used in:
/// * Keys of set expressions `{ <attr> = <expr>; .. }`.
/// * Set indexing expressions `set.index."another index"."interpolated ${index}"`.
///
/// Also note:
/// * The left-hand-side of `let <attr> = <expr>; .. in ..` bindings
///   syntactically also is an `Attr`, but gets desugared to a simple variable
///   instead. This turns `let a.b = 0; in ...` into
///   `let a = { b = 0; }; in ...`.
#[derive(Debug, Copy, Clone)]
pub struct Attr<'a> {
    /// Always contains at least one element.
    parts: &'a [AttrPart<'a>],
}

/// A segment of an attribute path.
///
/// Segments are separated by `.`.
#[derive(Debug)]
pub enum AttrPart<'a> {
    /// `unquoted_variable`
    Variable(Variable),
    /// `"quoted string"`
    String(&'a str),
}

pub struct Ast<'a> {
    root: &'a Expr<'a>,
    file: Arc<File>,
}

impl<'a> Ast<'a> {
    /// Builds a high-level AST from a raw expression parse tree.
    pub fn build<R: TreeRoot<Types>>(
        arenas: &'a Arenas,
        file: Arc<File>,
        search_path: &'a Path,
        root: rnix::parser::Node<R>,
    ) -> Result<Self, Error> {
        let root = {
            let mut builder = Builder::new(&file, search_path, arenas);
            builder.build(root)?
        };

        Ok(Self { root, file })
    }

    /// Returns the root expression represented by this AST.
    pub fn root(&self) -> &'a Expr<'a> {
        self.root
    }
}

impl<'a> fmt::Debug for Ast<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.root().fmt(f)
    }
}

/// The arena allocators providing the backing store for the AST.
///
/// We use one `copy_arena` for all `Copy` types that don't need drop logic, and
/// one `typed_arena` per type that *does* need `Drop` to be invoked.
pub struct Arenas {
    /// Arena for `Copy` types (most of the AST) that don't need drop logic.
    copy: CopyArena,

    /// Arena for `Value` instances.
    values: TypedArena<Value>,
}

impl Arenas {
    pub fn new() -> Self {
        Self {
            copy: CopyArena::new(),
            values: TypedArena::with_capacity(32),
        }
    }

    fn alloc<'a, T: ArenaBacked>(&'a self, t: T) -> &'a mut T {
        t.alloc_in_arena(self)
    }

    fn alloc_str(&self, s: &str) -> &str {
        self.copy.alloc_str(s)
    }
}

/// Trait implemented by all types that can be allocated in `Arenas`.
trait ArenaBacked {
    fn alloc_in_arena<'a>(self, arenas: &'a Arenas) -> &'a mut Self;
}

/// We can handle *all* `Copy` types easily.
impl<T: Copy> ArenaBacked for T {
    fn alloc_in_arena(self, arenas: &Arenas) -> &mut Self {
        arenas.copy.alloc(self)
    }
}

/// Types implementing `Drop` (or needing drop glue) need specialized arenas.
impl ArenaBacked for Value {
    fn alloc_in_arena(self, arenas: &Arenas) -> &mut Self {
        arenas.values.alloc(self)
    }
}
