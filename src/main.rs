use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read},
    str::FromStr,
};

use anyhow::{anyhow, bail, Result};
use chrono::prelude::*;
use rhai::{Dynamic, Engine, Scope, AST};
use serde::Deserialize;
use serde_repr::Deserialize_repr;

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

#[derive(Deserialize, Debug)]
struct BunyanEntry {
    v: i64,
    level: BunyanLevel,
    name: String,
    hostname: String,
    pid: u64,
    time: DateTime<Utc>,
    msg: String,

    /*
     * This is not a part of the base specification, but is widely used:
     */
    component: Option<String>,

    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Deserialize_repr, Debug, PartialEq, PartialOrd)]
#[repr(u8)]
enum BunyanLevel {
    Fatal = 60,
    Error = 50,
    Warn = 40,
    Info = 30,
    Debug = 20,
    Trace = 10,
}

impl FromStr for BunyanLevel {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        /*
         * We accept either the numeric value or the name (ignoring case) of a
         * level.  We also accept the four column wide truncated version of
         * names as they appear in some output formats; e.g., "DEBG" for Debug
         * level logs.
         */
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "60" | "fatal" | "fata" => BunyanLevel::Fatal,
            "50" | "error" | "erro" => BunyanLevel::Error,
            "40" | "warn" => BunyanLevel::Warn,
            "30" | "info" => BunyanLevel::Info,
            "20" | "debug" | "debg" => BunyanLevel::Debug,
            "10" | "trace" | "trac" => BunyanLevel::Trace,
            other => bail!("unknown level {:?}", other),
        })
    }
}

impl BunyanLevel {
    fn ansi_colour(&self, colour: Colour) -> String {
        match colour {
            Colour::None => "".to_string(),
            Colour::C16 => {
                let n = match self {
                    BunyanLevel::Fatal => 93,
                    BunyanLevel::Error => 91,
                    BunyanLevel::Warn => 95,
                    BunyanLevel::Info => 96,
                    BunyanLevel::Debug => 94,
                    BunyanLevel::Trace => 92,
                };
                format!("\x1b[{}m", n)
            }
            Colour::C256 => {
                let n = match self {
                    BunyanLevel::Fatal => 190,
                    BunyanLevel::Error => 160,
                    BunyanLevel::Warn => 130,
                    BunyanLevel::Info => 28,
                    BunyanLevel::Debug => 44,
                    BunyanLevel::Trace => 69,
                };
                format!("\x1b[38;5;{}m", n)
            }
        }
    }

    fn render(&self) -> &'static str {
        match self {
            BunyanLevel::Fatal => "FATA",
            BunyanLevel::Error => "ERRO",
            BunyanLevel::Warn => "WARN",
            BunyanLevel::Info => "INFO",
            BunyanLevel::Debug => "DEBG",
            BunyanLevel::Trace => "TRAC",
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

fn level(bl: BunyanLevel, colour: Colour) -> String {
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

fn emit_record(
    be: BunyanEntry,
    colour: Colour,
    fmt: Format,
    lookups: &Vec<String>,
) -> Result<()> {
    let l = level(be.level, colour);
    let mut n = bold(&be.name, colour);
    if matches!(fmt, Format::Long) {
        n += &format!("/{}", be.pid);
    }
    if let Some(c) = &be.component {
        if c != &be.name {
            n += &format!(" ({})", c);
        }
    };

    /*
     * For multi-line messages, indent subsequent lines by 4 spaces, so that
     * they are at least somewhat distinguishable from the next log message.
     */
    let msg = be
        .msg
        .lines()
        .enumerate()
        .map(|(i, l)| {
            let mut s = if i > 0 { "    " } else { "" }.to_string();
            s.push_str(l);
            s
        })
        .collect::<Vec<String>>()
        .join("\n");

    match fmt {
        Format::Short => {
            let d = be.time.format("%H:%M:%S%.3fZ").to_string();
            println!("{:13} {} {}: {}", d, l, n, msg);
        }
        Format::Long => {
            let d = be.time.format("%Y-%m-%d %H:%M:%S%.3fZ").to_string();
            println!("{} {} {} on {}: {}", d, l, n, be.hostname, msg);
        }
        Format::Bare => unreachable!(),
    }

    for (k, v) in be.extra.iter() {
        if !lookups.is_empty() && !lookups.contains(k) {
            continue;
        }

        print!("    {} = ", bold(k.as_str(), colour));

        match v {
            serde_json::Value::Null => println!("null"),
            serde_json::Value::Bool(v) => println!("{}", v),
            serde_json::Value::Number(n) => println!("{}", n),
            serde_json::Value::String(s) => {
                let mut out = String::new();
                for c in s.chars() {
                    if c != '"' && c != '\'' {
                        out.push_str(&c.escape_default().to_string());
                    } else {
                        out.push(c);
                    }
                }
                println!("{}", out);
            }
            serde_json::Value::Array(a) => println!("{:?}", a),
            serde_json::Value::Object(o) => println!("{:?}", o),
        }
    }

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

    let level =
        a.opt_str("l").as_deref().map(BunyanLevel::from_str).transpose()?;

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
                match serde_json::from_value::<BunyanEntry>(j.clone()) {
                    Ok(be) if be.v == 0 => {
                        if let Some(level) = &level {
                            if &be.level < level {
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
                            emit_record(be, colour, format, lookups)?;
                        }
                    }
                    Ok(_) => {
                        if matches!(format, Format::Bare) || filter.is_some() {
                            continue;
                        }

                        /*
                         * Unrecognised major version in this bunyan record.
                         */
                        println!("{}", l);
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
