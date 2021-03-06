use std::collections::BTreeMap;

use anyhow::Result;
use chrono::prelude::*;
use serde::Deserialize;
use serde_repr::Deserialize_repr;

#[derive(Clone, Copy)]
enum Colour {
    None,
    C16,
    C256,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
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

#[derive(Deserialize_repr, Debug)]
#[repr(u8)]
enum BunyanLevel {
    Fatal = 60,
    Error = 50,
    Warn = 40,
    Info = 30,
    Debug = 20,
    Trace = 10,
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

fn emit_record(be: BunyanEntry, colour: Colour) -> Result<()> {
    let d = be.time.format("%H:%M:%S%.3fZ").to_string();
    let l = level(be.level, colour);
    let mut n = bold(&be.name, colour);
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

    println!("{:13} {} {}: {}", d, l, n, msg);
    for (k, v) in be.extra.iter() {
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

fn main() -> Result<()> {
    let stdin = std::io::stdin();
    let mut lines = stdin.lines();

    let mut opts = getopts::Options::new();
    opts.optflag("C", "", "force coloured output when not a tty");
    opts.optflag("N", "", "no terminal formatting");

    let a = opts.parse(std::env::args().skip(1))?;

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
                    Ok(be) if be.v == 0 => emit_record(be, colour)?,
                    Ok(_) => {
                        /*
                         * Unrecognised major version in this bunyan record.
                         */
                        println!("{}", l);
                    }
                    Err(_) => {
                        /*
                         * This record does not contain the minimum required
                         * fields.
                         */
                        println!("{}", l);
                    }
                }
            }
            Err(_) => {
                /*
                 * Lines that cannot be parsed as JSON are emitted as-is.
                 */
                println!("{}", l);
            }
        }
    }

    Ok(())
}
