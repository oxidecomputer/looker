use std::{
    io::{BufRead, BufReader, Read},
    str::FromStr,
};

use anyhow::{anyhow, bail, Result};
use bunyan::BunyanEntry;
use rhai::{Dynamic, Engine, Scope, AST};
use serde::Deserialize;
use serde_repr::Deserialize_repr;
use tracing::TracingEntry;

mod bunyan;
mod tracing;

#[derive(Clone, Copy)]
enum Format {
    Short,
    Long,
    Bare,
}

#[derive(Clone, Copy)]
enum Colour {
    None,
    C16,
    C256,
}

#[derive(Copy, Clone, Deserialize_repr, Debug, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum Level {
    Fatal = 60,
    Error = 50,
    Warn = 40,
    Info = 30,
    Debug = 20,
    Trace = 10,
}

impl FromStr for Level {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        /*
         * We accept either the numeric value or the name (ignoring case) of a
         * level.  We also accept the four column wide truncated version of
         * names as they appear in some output formats; e.g., "DEBG" for Debug
         * level logs.
         */
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "60" | "fatal" | "fata" => Level::Fatal,
            "50" | "error" | "erro" => Level::Error,
            "40" | "warn" => Level::Warn,
            "30" | "info" => Level::Info,
            "20" | "debug" | "debg" => Level::Debug,
            "10" | "trace" | "trac" => Level::Trace,
            other => bail!("unknown level {:?}", other),
        })
    }
}

impl Level {
    fn ansi_colour(&self, colour: Colour) -> String {
        match colour {
            Colour::None => "".to_string(),
            Colour::C16 => {
                let n = match self {
                    Level::Fatal => 93,
                    Level::Error => 91,
                    Level::Warn => 95,
                    Level::Info => 96,
                    Level::Debug => 94,
                    Level::Trace => 92,
                };
                format!("\x1b[{}m", n)
            }
            Colour::C256 => {
                let n = match self {
                    Level::Fatal => 190,
                    Level::Error => 160,
                    Level::Warn => 130,
                    Level::Info => 28,
                    Level::Debug => 44,
                    Level::Trace => 69,
                };
                format!("\x1b[38;5;{}m", n)
            }
        }
    }

    fn render(&self) -> &'static str {
        match self {
            Level::Fatal => "FATA",
            Level::Error => "ERRO",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBG",
            Level::Trace => "TRAC",
        }
    }
}

fn bold(input: &str, colour: Colour) -> String {
    let fancy = !matches!(colour, Colour::None);
    let mut s = "".to_string();
    if fancy {
        s += "\x1b[1m";
    }
    s += input;
    if fancy {
        s += "\x1b[0m";
    }
    s
}

fn level(bl: Level, colour: Colour) -> String {
    bold(&format!("{}{}", bl.ansi_colour(colour), bl.render()), colour)
}

fn emit_bare(j: serde_json::Value, lookups: &Vec<String>) -> Result<()> {
    let o = j.as_object().unwrap();
    let mut outs = Vec::new();
    for l in lookups {
        if let Some(v) = o.get(l) {
            outs.push(match v {
                serde_json::Value::Null => "null".to_string(),
                serde_json::Value::Bool(v) => format!("{}", v),
                serde_json::Value::Number(n) => format!("{}", n),
                serde_json::Value::String(s) => {
                    let mut out = String::new();
                    for c in s.chars() {
                        if c != '"' && c != '\'' {
                            out.push_str(&c.escape_default().to_string());
                        } else {
                            out.push(c);
                        }
                    }
                    out
                }
                serde_json::Value::Array(a) => format!("{:?}", a),
                serde_json::Value::Object(o) => format!("{:?}", o),
            });
        } else {
            outs.push("-".into());
        }
    }

    println!("{}", outs.join(" "));
    Ok(())
}

fn guess_colour_depth(try_hard: bool) -> Colour {
    match std::env::var("TERM") {
        Ok(term) => {
            if term.contains("256") {
                Colour::C256
            } else if !try_hard && term == "dumb" {
                Colour::None
            } else {
                Colour::C16
            }
        }
        Err(_) => {
            if try_hard {
                Colour::C16
            } else {
                Colour::None
            }
        }
    }
}

struct Filter<'a> {
    engine: Engine,
    ast: AST,
    scope: Scope<'a>,
}

fn parse_filter(s: String) -> Result<Filter<'static>> {
    let mut engine = Engine::new();
    engine.register_fn("as_int", |d: Dynamic| -> Dynamic {
        if d.is_unit() {
            /*
             * Just propagate this error.
             */
            Dynamic::UNIT
        } else if d.is_int() {
            /*
             * Pass an integer through unmodified.
             */
            d
        } else if let Ok(s) = d.into_string() {
            s.parse::<i64>().ok().map(|n| n.into()).unwrap_or(Dynamic::UNIT)
        } else {
            Dynamic::UNIT
        }
    });
    let scope = Scope::new();
    let ast = engine
        .compile_into_self_contained(&scope, s)
        .map_err(|e| anyhow!("compiling script: {e}"))?;

    Ok(Filter { engine, ast, scope })
}

trait Record {
    fn level(&self) -> Level;

    fn emit_record(
        &self,
        colour: Colour,
        fmt: Format,
        lookups: &Vec<String>,
    ) -> Result<()>;
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Entry {
    Bunyan(BunyanEntry),
    Tracing(TracingEntry),
}

impl Entry {
    fn as_record(&self) -> &dyn Record {
        match self {
            Entry::Bunyan(be) => be,
            Entry::Tracing(tr) => tr,
        }
    }
}

fn main() -> Result<()> {
    let mut opts = getopts::Options::new();
    opts.optflag("", "help", "usage information");
    opts.optflag("C", "", "force coloured output when not a tty");
    opts.optflag("N", "", "no terminal formatting");
    opts.optopt(
        "l",
        "level",
        "only show messages at or above this level\n\
        (e.g., \"info\" or \"30\")",
        "LEVEL",
    );
    opts.optopt(
        "o",
        "output",
        "output format:\n\
        - \"short\" is the default output format\n\
        - \"long\" prints all fields and long timestamps\n",
        "FORMAT",
    );
    opts.optopt(
        "c",
        "",
        "filter the input with a rhai script that returns a \
        boolean expression: true to include or false to elide; \
        use `r` to refer to the record under consideration",
        "SCRIPT",
    );
    opts.optopt("f", "", "read input from a file rather than stdin", "FILE");

    let a = match opts.parse(std::env::args().skip(1)) {
        Ok(a) => {
            if a.opt_present("help") {
                println!("{}", opts.usage(opts.short_usage("looker").trim()));
                return Ok(());
            }
            a
        }
        Err(e) => {
            eprintln!("{}\nERROR: {}", opts.short_usage("looker"), e);
            std::process::exit(1);
        }
    };

    let input: Box<dyn Read> = if let Some(p) = a.opt_str("f") {
        Box::new(
            std::fs::File::open(&p)
                .map_err(|e| anyhow!("opening file {p:?}: {e}"))?,
        )
    } else {
        if atty::is(atty::Stream::Stdin) {
            /*
             * It is unlikely that the user intended to run the command without
             * directing a file or pipe as input.
             */
            eprintln!("WARNING: reading from stdin, which is a tty");
        }

        Box::new(std::io::stdin())
    };
    let input = BufReader::new(input);
    let mut lines = input.lines();

    let mut filter = a.opt_str("c").map(parse_filter).transpose()?;

    let lookups = &a.free;

    let format = match a.opt_str("o").as_deref() {
        Some("short") | None => Format::Short,
        Some("long") => Format::Long,
        Some("bare") => {
            if lookups.is_empty() {
                bail!("bare mode requires at least one property to print");
            }

            Format::Bare
        }
        Some(other) => {
            eprintln!(
                "{}\nERROR: unknown format type {:?}",
                opts.short_usage("looker"),
                other,
            );
            std::process::exit(1);
        }
    };

    let level = a.opt_str("l").as_deref().map(Level::from_str).transpose()?;

    let colour = if a.opt_present("N") {
        Colour::None
    } else if a.opt_present("C") || atty::is(atty::Stream::Stdout) {
        /*
         * If explicitly enabled, or if we are interactive, try to use colours:
         */
        guess_colour_depth(a.opt_present("C"))
    } else {
        Colour::None
    };

    while let Some(l) = lines.next().transpose()? {
        match serde_json::from_str::<serde_json::Value>(&l) {
            Ok(j) => {
                match serde_json::from_value::<Entry>(j.clone()) {
                    Ok(Entry::Bunyan(be)) if be.v != 0 => {
                        if matches!(format, Format::Bare) || filter.is_some() {
                            continue;
                        }

                        /*
                         * Unrecognised major version in this bunyan record.
                         */
                        println!("{}", l);
                    }
                    Ok(entry) => {
                        let record = entry.as_record();

                        if let Some(level) = &level {
                            if &record.level() < level {
                                continue;
                            }
                        }

                        if let Some(filter) = &mut filter {
                            let r: Dynamic = serde_json::from_value(j.clone())?;

                            filter.scope.set_or_push("r", r);

                            let include = filter
                                .engine
                                .eval_ast_with_scope::<Dynamic>(
                                    &mut filter.scope,
                                    &filter.ast,
                                )
                                .map_err(|e| anyhow!("script error: {e}"))?;

                            let include = if include.is_unit() {
                                /*
                                 * If a script returns (), for convenience
                                 * we treat that as a request to elide the
                                 * record.  This makes it possible to do
                                 * things like:
                                 *
                                 *  r.component?.contains("dropshot")
                                 */
                                false
                            } else if let Ok(include) = include.as_bool() {
                                include
                            } else {
                                bail!(
                                    "script returned type {:?}, \
                                    not a bool or ()",
                                    include.type_name()
                                );
                            };

                            if !include {
                                continue;
                            }
                        }

                        if matches!(format, Format::Bare) {
                            emit_bare(j, lookups)?;
                        } else {
                            record.emit_record(colour, format, lookups)?;
                        }
                    }
                    Err(_) => {
                        if matches!(format, Format::Bare) || filter.is_some() {
                            continue;
                        }

                        /*
                         * This record does not contain the minimum required
                         * fields.
                         */
                        println!("{}", l);
                    }
                }
            }
            Err(_) => {
                if matches!(format, Format::Bare) || filter.is_some() {
                    continue;
                }

                /*
                 * Lines that cannot be parsed as JSON are emitted as-is.
                 */
                println!("{}", l);
            }
        }
    }

    Ok(())
}
