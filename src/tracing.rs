use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::{Record, Colour, Format, Level, level, bold};

#[derive(Deserialize, Debug)]
pub struct TracingEntry {
    pub timestamp: DateTime<Utc>,
    pub level: TracingLevel,
    pub target: String,
    pub fields: Fields,
    pub span: Option<Span>,
    pub spans: Option<Vec<Span>>,
}

#[derive(Copy, Clone, Deserialize, Debug)]
#[serde(rename_all = "UPPERCASE")]
pub enum TracingLevel {
    Debug,
    Error,
    Info,
    Warn,
    Trace,
}

#[derive(Deserialize, Debug)]
pub struct Fields {
    pub message: String,
    #[serde(flatten)]
    pub values: BTreeMap<String, Value>,
}

#[derive(Deserialize, Debug)]
pub struct Span {
    pub name: String,
    #[serde(flatten)]
    pub values: BTreeMap<String, Value>,
}

impl From<TracingLevel> for Level {
    fn from(value: TracingLevel) -> Self {
        match value {
            TracingLevel::Debug => Level::Debug,
            TracingLevel::Error => Level::Error,
            TracingLevel::Info => Level::Info,
            TracingLevel::Warn => Level::Warn,
            TracingLevel::Trace => Level::Trace,
        }
    }
}

impl Record for TracingEntry {
    fn level(&self) -> Level {
        self.level.into()
    }

    fn emit_record(
            &self,
            colour: Colour,
            fmt: Format,
            lookups: &Vec<String>,
        ) -> anyhow::Result<()> {
        let l = level(self.level.into(), colour);
        let n = bold(&self.target, colour);

        /*
            * For multi-line messages, indent subsequent lines by 4 spaces, so that
            * they are at least somewhat distinguishable from the next log message.
            */
        let msg = self
            .fields
            .message
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
                let d = self.timestamp.format("%H:%M:%S%.3fZ").to_string();
                println!("{:13} {} {}: {}", d, l, n, msg);
            }
            Format::Long => {
                let d = self.timestamp.format("%Y-%m-%d %H:%M:%S%.3fZ").to_string();
                println!("{} {} {}: {}", d, l, n, msg);
            }
            Format::Bare => unreachable!(),
        }

        for (k, v) in self.fields.values.iter() {
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

        if let Some(spans) = &self.spans {
            for (i, span) in spans.iter().enumerate() {
                span.emit_span(&format!("span[{}]", i), colour, lookups);
            }
        }

        Ok(())
    }
}

impl Span {
    fn emit_span(
        &self,
        prefix: &str,
        colour: Colour,
        lookups: &Vec<String>
    ) {
        for (k, v) in self.values.iter() {
            if !lookups.is_empty() && !lookups.contains(k) {
                continue;
            }

            print!("    {}::{} = ", bold(prefix, colour), bold(k.as_str(), colour));

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
    }
}