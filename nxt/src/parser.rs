//! Nix expression parser using `rnix`.
//!
//! The actual parsing is done by `rnix`, but this module implements a few
//! useful helpers to deal with parse error extraction and rendering.

use profile::profile;

use codemap::{File, Span, SpanLoc};
use codemap_diagnostic::{Diagnostic, Level, SpanLabel, SpanStyle};
use rnix::parser::{NodeType, ParseError, Types, AST};
use rnix::tokenizer::Token;
use rnix::types::*;
use rowan::{OwnedRoot, TreeRoot};
use std::fmt;
use std::sync::Arc;

#[derive(Debug)]
pub struct Error {
    span: Span,
    span_loc: SpanLoc,
    message: String,
}

impl Error {
    pub fn at<N, M, R>(source: Arc<File>, node: &N, message: M) -> Self
    where
        N: TypedNode<R>,
        M: Into<String>,
        R: TreeRoot<Types>,
    {
        let range = node.node().range();
        let span = source.span.subspan(
            range.start().to_usize() as u64,
            range.end().to_usize() as u64,
        );

        Self {
            span_loc: SpanLoc {
                begin: source.find_line_col(span.low()),
                end: source.find_line_col(span.high()),
                file: source,
            },
            span,
            message: message.into(),
        }
    }

    fn from_inner(source: Arc<File>, error: ParseError) -> Self {
        let span = error_span(&source, &error);

        Self {
            span_loc: SpanLoc {
                begin: source.find_line_col(span.low()),
                end: source.find_line_col(span.high()),
                file: source,
            },
            span,
            message: error_fmt(&error),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "syntax error at {}: {}", self.span_loc, self.message)
    }
}

impl Into<Diagnostic> for Error {
    fn into(self) -> Diagnostic {
        Diagnostic {
            level: Level::Error,
            message: format!("could not parse {}", self.span_loc.file.name()),
            code: None,
            spans: vec![SpanLabel {
                span: self.span,
                label: Some(self.message),
                style: SpanStyle::Primary,
            }],
        }
    }
}

/// Takes a list of `ParseErrors` and tries to extract a human-readable parsing
/// error.
///
/// `rnix` will most likely return a whole list of errors, many of which just
/// say "unexpected end of file" even though that's not really accurate. That's
/// why this code ranks errors based on their type and other available
/// information, and then sorts the list of errors to obtain the error that's
/// most likely to be of highest value to a user.
fn extract_error(source: &Arc<File>, mut errors: Vec<ParseError>) -> Result<(), Error> {
    fn rank_error(e: &ParseError) -> u32 {
        match e {
            ParseError::Unexpected(node) => {
                // The node might point to an empty part of the input, so give those a low rank
                if node.range().is_empty() {
                    1
                } else {
                    2
                }
            }
            ParseError::UnexpectedEOFWanted(_) => 1, // at least contains the expected token
            ParseError::UnexpectedEOF => 0,          // almost no information
        }
    }

    if errors.is_empty() {
        return Ok(());
    }

    errors.sort_by_key(rank_error); // (should be a stable sort)

    trace!("{} errors:", errors.len());
    for error in &errors {
        trace!("{}", Error::from_inner(source.clone(), error.clone()));
    }

    // Pick the best error, or the first "good" error
    let error = errors.pop().unwrap();
    Err(Error::from_inner(source.clone(), error))
}

fn error_fmt(error: &ParseError) -> String {
    match error {
        ParseError::Unexpected(node) => format!("unexpected input near `{}`", node),
        ParseError::UnexpectedEOF => "unexpected end of input".into(),
        ParseError::UnexpectedEOFWanted(token) => {
            // FIXME impl display for token
            format!("unexpected end of input (expected {:?})", token)
        }
    }
}

fn error_span(source: &File, error: &ParseError) -> Span {
    match error {
        ParseError::Unexpected(node) => {
            // convert range to span
            let range = node.range();
            source.span.subspan(
                range.start().to_usize() as u64,
                range.end().to_usize() as u64,
            )
        }
        ParseError::UnexpectedEOFWanted(_) | ParseError::UnexpectedEOF => {
            // put the span at the end of the input
            source.span.subspan(source.span.len(), source.span.len())
        }
    }
}

/// Parses a Nix expression.
pub fn parse(file: &Arc<File>) -> Result<RawExpr<OwnedRoot<Types>>, Error> {
    profile("parsing", file.name(), || parse_impl(file))
}

fn parse_impl(expr: &Arc<File>) -> Result<RawExpr<OwnedRoot<Types>>, Error> {
    let ast = rnix::parse(expr.source());

    extract_error(expr, ast.errors())?;

    Ok(RawExpr::from_raw(ast.into_node().first_child().unwrap()))
}

/// A raw expression parse tree.
///
/// This is a slim wrapper around `rnix`'s types that does the downcasting for
/// you.
pub enum RawExpr<R: TreeRoot<Types>> {
    Apply(Apply<R>),
    Assert(Assert<R>),
    /// An identifier.
    Ident(Ident<R>),
    IfElse(IfElse<R>),
    /// `set.index`
    IndexSet(IndexSet<R>),
    Lambda(Lambda<R>),
    LetIn(LetIn<R>),
    List(List<R>),
    /// Binary operation.
    Operation(Operation<R>),
    Unary(Unary<R>),
    /// `set.index or def`
    OrDefault(OrDefault<R>),
    /// Parenthesized expression.
    Paren(Paren<R>), // XXX remove?
    Set(Set<R>),
    /// A literal value.
    Value(rnix::types::Value<R>),
    /// `with e1; e2`
    With(With<R>),
}

impl<R: TreeRoot<Types>> RawExpr<R> {
    /// Converts a raw AST node to an `Expr` node.
    ///
    /// If `node` is not a valid expression node, this function will panic.
    pub fn from_raw(node: rnix::parser::Node<R>) -> Self {
        macro_rules! match_expr {
            ( $($node:tt),* else($elvar:ident) $el:expr ) => {
                match node.kind() {
                    $(
                    NodeType::$node => RawExpr::$node($node::cast(node).unwrap()),
                    )*

                    $elvar => $el
                }
            };
        }

        match_expr!(
            Apply, Assert, IfElse, IndexSet, Lambda, LetIn, List, Operation, Unary,
            OrDefault, Paren, Set, With

            else(kind) {
                match kind {
                    // Ident = NodeType::Token(Token::Ident(_))
                    NodeType::Token(Token::Ident) => RawExpr::Ident(Ident::cast(node).unwrap()),
                    // Value = ???
                    NodeType::Token(t) if t.is_value() => RawExpr::Value(Value::cast(node).unwrap()),
                    _ => panic!("unexpected AST node kind: {:?} (expected expression)", kind),
                }
            }
        )
    }
}
