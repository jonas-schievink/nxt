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

/*

AST building:

* Arena-backed
* Fully visit every node that introduces a new variable scope, and resolve all
  idents. Any mention of an undeclared variable is actually an error in Nix, so
  it must be an error here too.

*/

/// An expression.
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

    Lambda {
        argument: (),   // complex due to pattern matching
        body: &'a Expr<'a>,
    },

    /// `let <bindings> in <body>`
    ///
    /// Binds expressions to variables/attributes, creating a new scope in which
    /// `body` is evaluated. Existing variables are shadowed.
    ///
    /// Note that all bound variables can be referred to by other bindings,
    /// potentially allowing infinite recursion (like recursive sets).
    LetIn {
        /// "Desugared" bindings.
        bindings: &'a [(Variable, Expr<'a>)],
        body: &'a Expr<'a>,
    },
}

pub struct Variable(u32);

/// An attribute or variable path.
///
/// `a`, `"a"."a"`, `x.y`, `x."${interpolated} string"`.
///
/// An attribute making use of `"${interpolation}"` is also called "dynamic
/// attribute" and is not allowed in `let .. in ..` bindings.
///
/// This is used in:
/// * Keys of set expressions `{ <attr> = <expr>; .. }`.
/// * The left-hand-side of `let <attr> = <expr>; .. in ..` bindings.
/// * Set indexing expressions `set.index."another index"."interpolated ${index}"`.
pub struct Attr<'a> {
    /// Always contains at least one element.
    parts: &'a [AttrPart],
}

pub enum AttrPart<'a> {
    /// `unquoted_variable`
    Variable(Variable),
    /// `"quoted string"`
    String(&'a str),
}
