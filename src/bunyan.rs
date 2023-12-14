use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::{Level, Record, level, bold, Format};

#[derive(Deserialize, Debug)]
pub struct BunyanEntry {
    pub v: i64,
    pub level: Level,
    pub name: String,
    pub hostname: String,
    pub pid: u64,
    pub time: DateTime<Utc>,
    pub msg: String,

    /*
     * This is not a part of the base specification, but is widely used:
     */
    pub component: Option<String>,

    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Record for BunyanEntry {
    fn level(&self) -> Level {
        self.level
    }

    fn emit_record(
            &self,
            colour: crate::Colour,
            fmt: crate::Format,
            lookups: &Vec<String>,
    ) -> anyhow::Result<()> {
        let l = level(self.level, colour);
        let mut n = bold(&self.name, colour);
        if matches!(fmt, Format::Long) {
            n += &format!("/{}", self.pid);
        }
        if let Some(c) = &self.component {
            if c != &self.name {
                n += &format!(" ({})", c);
            }
        };

        /*
            * For multi-line messages, indent subsequent lines by 4 spaces, so that
            * they are at least somewhat distinguishable from the next log message.
            */
        let msg = self
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
                let d = self.time.format("%H:%M:%S%.3fZ").to_string();
                println!("{:13} {} {}: {}", d, l, n, msg);
            }
            Format::Long => {
                let d = self.time.format("%Y-%m-%d %H:%M:%S%.3fZ").to_string();
                println!("{} {} {} on {}: {}", d, l, n, self.hostname, msg);
            }
            Format::Bare => unreachable!(),
        }

        for (k, v) in self.extra.iter() {
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
}
