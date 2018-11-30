use super::*;
use parser::RawExpr;
use rnix::value::{self, ValueError};

use codemap::File;
use directories::BaseDirs;
use hashbrown::hash_map::Entry;
use hashbrown::HashMap;
use rnix::parser::Types;
use rnix::value::Anchor;
use rowan::TreeRoot;
use std::path::Path;
use tendril::StrTendril;

/// The AST builder builds our simplified AST from a parse tree.
pub struct Builder<'arenas, 'a> {
    arenas: &'arenas Arenas,
    file: &'a Arc<File>,
    search_path: &'a Path,
    /// The current scope stack.
    scopes: Vec<Scope>,
    /// Maps `Variable` IDs to their `VarInfo`.
    variables: Vec<VarInfo<'arenas>>,
}

impl<'arenas, 'a> Builder<'arenas, 'a> {
    /// Creates a new AST builder.
    ///
    /// # Parameters
    ///
    /// * `file`: Source file being processed.
    /// * `search_path`: Path to prepend to relative paths (`./xyz`). This
    ///   should be the directory containing the source file, or the current
    ///   working directory if no real file is processed.
    /// * `arenas`: Arenas to allocate AST nodes and data in.
    pub fn new(file: &'a Arc<File>, search_path: &'a Path, arenas: &'arenas Arenas) -> Self {
        let mut this = Self {
            arenas,
            file,
            search_path,
            scopes: vec![Scope::empty()],
            variables: vec![],
        };

        this.define_variable(VarInfo {
            decl_span: file.span.subspan(0, 0),
            name: "true",
            value: this
                .arenas
                .alloc(Expr::Value(this.arenas.alloc(Value::Bool(true)))),
        }).unwrap();
        this.define_variable(VarInfo {
            decl_span: file.span.subspan(0, 0),
            name: "false",
            value: this
                .arenas
                .alloc(Expr::Value(this.arenas.alloc(Value::Bool(false)))),
        }).unwrap();
        this
    }

    pub fn build<R: TreeRoot<Types>>(
        &mut self,
        root: rnix::parser::Node<R>,
    ) -> Result<&'arenas Expr<'arenas>, Error> {
        self.translate_expr(root)
    }

    fn translate_expr<R: TreeRoot<Types>>(
        &mut self,
        expr: rnix::parser::Node<R>,
    ) -> Result<&'arenas Expr<'arenas>, Error> {
        match RawExpr::from(expr) {
            RawExpr::Value(v) => {
                let value = v.to_value().map_err(|e| {
                    let msg = match e {
                        ValueError::Float(err) => format!("invalid float: {}", err),
                        ValueError::Integer(err) => format!("invalid integer: {}", err),
                        ValueError::String => format!("invalid string"),
                        ValueError::StorePath => format!("invalid store path"),
                        ValueError::Unknown => format!("invalid literal"),
                    };
                    Error::at(self.file.clone(), &v, msg)
                })?;

                // Convert rnix's `Value` to our `Value`
                let value = match value {
                    value::Value::Float(f) => Value::Float(f),
                    value::Value::Integer(i) => Value::Int(i),
                    value::Value::Str {
                        content,
                        multiline: _,
                    }
                    | value::Value::Path(Anchor::Uri, content) => Value::String(content.into()),
                    value::Value::Path(anchor, path) => match anchor {
                        Anchor::Absolute => Value::Path(path.into()),
                        // Turn relative paths absolute by prepending the search dir
                        Anchor::Relative => {
                            let mut full_path = self.search_path.to_path_buf();
                            full_path.push(path);
                            Value::Path(full_path)
                        }
                        Anchor::Home => {
                            let mut base = BaseDirs::new()
                                .expect("failed to retrieve home path")
                                .home_dir()
                                .to_path_buf();
                            base.push(path);
                            Value::Path(base)
                        }
                        Anchor::Store => {
                            // `<path>`-style paths are resolved lazily, so they're actually `Expr`s
                            // instead of `Value`s.
                            let path = Path::new(self.arenas.alloc_str(&path));
                            return Ok(self.arenas.alloc(Expr::NixPath(path)));
                        },
                        Anchor::Uri => unreachable!(), // handled above
                    },
                };

                Ok(self.arenas.alloc(Expr::Value(self.arenas.alloc(value))))
            }
            RawExpr::Apply(_) => unimplemented!(),
            RawExpr::Assert(assert) => {
                let cond = self.translate_expr(assert.condition())?;
                let body = self.translate_expr(assert.body())?;

                Ok(self.arenas.alloc(Expr::Assert {
                    assertion: cond,
                    then: body,
                }))
            }
            RawExpr::Ident(ident) => {
                let var = self.resolve_local_variable(ident.as_str()).map_err(|()| {
                    Error::at(self.file.clone(), &ident, "cannot resolve variable")
                })?;
                Ok(self.arenas.alloc(Expr::Variable(var)))
            }
            RawExpr::IfElse(_) => unimplemented!(),
            RawExpr::IndexSet(_) => unimplemented!(),
            RawExpr::Lambda(_) => unimplemented!(),
            RawExpr::LetIn(_) => unimplemented!(),
            RawExpr::List(_) => unimplemented!(),
            _ => unimplemented!(),
        }
    }

    /// Resolves a named local variable to a `Variable` ID.
    ///
    /// This is a very hashmap-heavy operation, since it interns the identifier
    /// and walks up the scope stack.
    fn resolve_local_variable(&mut self, name: &str) -> Result<Variable, ()> {
        let (innermost, rest) = self.scopes.split_last_mut().expect("no scope");
        let tendril = StrTendril::from(name);
        match innermost.entries.entry(tendril) {
            Entry::Occupied(entry) => Ok(*entry.get()),
            Entry::Vacant(entry) => {
                // Slow path: Walk scope stack upwards
                let tendril = StrTendril::from(name);
                for scope in rest.iter_mut().rev() {
                    if let Some(&variable) = scope.entries.get(&tendril) {
                        entry.insert(variable);
                        return Ok(variable);
                    }
                }

                Err(())
            }
        }
    }

    /// Defines a new local variable in the currently active scope.
    ///
    /// If the scope already defines a variables named `name`, this returns an
    /// error.
    fn define_variable(&mut self, var: VarInfo<'arenas>) -> Result<Variable, ()> {
        let variable = Variable(self.variables.len() as u32);
        let name = StrTendril::from(var.name);

        match self
            .scopes
            .last_mut()
            .expect("empty scope stack, this should never happen")
            .entries
            .entry(name)
        {
            Entry::Occupied(_) => {
                return Err(());
            }
            Entry::Vacant(vacant) => {
                vacant.insert(variable);
                self.variables.push(var);
                Ok(variable)
            }
        }
    }
}

/// A variable scope.
pub struct Scope {
    /// Variable entries.
    ///
    /// An entry in this hash map does *not* necessarily mean that the variable
    /// was defined in this scope: We also store an entry when searching the
    /// scope stack for an outer variable to prevent redundant lookups in the
    /// future.
    entries: HashMap<StrTendril, Variable>,
}

impl Scope {
    fn empty() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}
