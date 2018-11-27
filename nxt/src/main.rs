#![doc(html_root_url = "https://docs.rs/nxt/0.1.0")]
#![warn(missing_debug_implementations)]

#[macro_use]
extern crate log;
#[macro_use]
extern crate failure;
extern crate codemap;
extern crate codemap_diagnostic;
extern crate console;
extern crate directories;
extern crate env_logger;
extern crate hashbrown;
extern crate num_traits;
extern crate rnix;
extern crate rowan;
extern crate shawshank;
extern crate structopt;
extern crate tendril;
extern crate toolshed;
extern crate typed_arena;

mod ast;
mod config;
mod eval;
mod parser;
mod profile;
mod utils;
mod value;

use failure::Error;
use log::LevelFilter;
use structopt::StructOpt;

use config::Config;
use eval::EvalContext;
use eval::Source;
use std::cmp;
use std::env;
use std::process::exit;

#[derive(StructOpt)]
#[structopt(about = "A Nix expression evaluator")]
struct Opts {
    #[structopt(parse(from_occurrences))]
    #[structopt(short = "v")]
    #[structopt(
        help = "\
                Increase verbosity:\n\
                -v    Print debugging messages\n\
                -vv   Print tracing messages\
                "
    )]
    verbosity: u8,

    #[structopt(parse(from_occurrences))]
    #[structopt(short = "q")]
    #[structopt(
        help = "\
                Decrease verbosity:\n\
                -q    Only print warnings and errors\n\
                -qq   Only print errors\n\
                -qqq  Do not print anything\
                "
    )]
    quiet: u8,

    /// Collect and log timing data for internal operations.
    #[structopt(long = "profile")]
    profile: bool,

    /// When to use colored console output (always, never, or auto).
    #[structopt(long = "color", default_value = "auto")]
    color: utils::ColorConfig,

    #[structopt(flatten)]
    cmd: Subcommand,
}

#[derive(StructOpt)]
enum Subcommand {
    #[structopt(name = "eval")]
    #[structopt(about = "Evaluate a Nix expression")]
    Eval {
        /// The expression to evaluate.
        expr: String,
    },
}

fn run(opts: Opts) -> Result<(), Error> {
    if opts.verbosity > 0 && opts.quiet > 0 {
        bail!("cannot specify -v and -q at the same time");
    }

    // Build the right LevelFilter, defaulting to `Info`
    // Off  Error  Warn  Info  Debug  Trace
    //  0     1     2     3      4      5
    let verbosity = cmp::min(opts.verbosity, 2);
    let quiet = cmp::min(opts.quiet, 3);
    let level = 3 - quiet + verbosity;
    let filter = match level {
        0 => LevelFilter::Off,
        1 => LevelFilter::Error,
        2 => LevelFilter::Warn,
        3 => LevelFilter::Info,
        4 => LevelFilter::Debug,
        5 => LevelFilter::Trace,
        _ => unreachable!(),
    };
    // Enable the selected level for this crate only:
    env_logger::Builder::from_default_env()
        .filter(None, filter)
        .init();
    debug!("logging enabled at {:?} level", filter);

    if opts.profile {
        profile::enable();
    }

    let config = Config { color: opts.color };

    match opts.cmd {
        Subcommand::Eval { expr } => {
            let mut eval = EvalContext::new(config);
            let value = eval.eval(Source::Other {
                source: &expr,
                name: "<cmdline>",
                search_path: &env::current_dir()?,
            })?;

            println!("{}", value);

            Ok(())
        }
    }
}

fn main() {
    let opts = Opts::from_args();

    match run(opts) {
        Ok(()) => {}
        Err(e) => {
            if e.downcast_ref::<utils::ErrorAlreadyPrinted>().is_none() {
                eprintln!("error: {}", e);
            }
            exit(1);
        }
    }
}
