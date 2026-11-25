//! The command line: `adsertio <command> --flag value ...`.
//!
//! Commands are the MCP tools without the `adsertio_` prefix (claim, view), plus
//! init, check, mcp. Flags come from the same parameter table as
//! the MCP schema. An unknown flag, a repeated flag, a flag with no value, or
//! a bad number is a usage error (exit 2); nothing is guessed. A value that
//! starts with `--` is passed as `--name=value` or `--name -- value`.

use crate::json::Value;
use crate::tool::{Kind, Tool};
use crate::{store::Store, tools, SERVER, VERSION};

const HELP: &str = "adsertio: one verified-or-unverified view per claim

USAGE
  adsertio init                       create .adsertio/ in this folder
  adsertio claim --summary S [--evidence tool:path[#selector]]... [--author A] [--body B] [--id I] [--tags T]...
  adsertio attach --id I --evidence tool:path[#selector]...
  adsertio view [--id I] [--verdict verified|refuted|unverified] [--states S]... [--compact] [--limit N] [--offset N] [--after CURSOR]
  adsertio ls [--limit N] [--offset N]
  adsertio guide
  adsertio check                      read every claim; fail on any damage
  adsertio mcp                        start the stdio MCP server (same as adsertio-mcp)
  adsertio --version

A value that starts with -- goes after =, as in --summary=--fast, or after a
lone --, as in --summary -- --fast. Each flag is given once, except --evidence,
--tags and --states, which repeat.

Evidence tools: trammel, chisel, probatio. Output is JSON; the exit code is 1 on
any error, 2 on a usage error (an unknown command or flag, a repeated flag, a
missing or bad value).";

pub fn run(args: &[String], as_mcp: bool) -> i32 {
    if as_mcp {
        return crate::mcp::serve(&SERVER);
    }
    let Some(cmd) = args.first() else {
        eprintln!("{HELP}");
        return 2;
    };
    match cmd.as_str() {
        "--version" | "-V" | "version" => {
            println!("{{\"name\":\"adsertio\",\"version\":\"{VERSION}\"}}");
            0
        }
        "--help" | "-h" | "help" => {
            println!("{HELP}");
            0
        }
        "mcp" | "serve-mcp" => crate::mcp::serve(&SERVER),
        "init" => finish(
            Store::init()
                .map(|s| Value::obj(vec![("store", Value::String(s.dir.display().to_string()))])),
        ),
        "check" => finish(crate::tools::store_check()),
        other => match tools::TOOLS
            .iter()
            .find(|t| t.name.strip_prefix("adsertio_") == Some(other))
        {
            None => {
                eprintln!("unknown command \"{other}\"\n\n{HELP}");
                2
            }
            Some(tool) => match build_args(tool, &args[1..]) {
                Err(e) => {
                    eprintln!("{e}");
                    2
                }
                Ok(v) => {
                    let out = tool.validate(&v).and_then(|a| (tool.run)(&a));
                    finish(out)
                }
            },
        },
    }
}

fn finish(r: Result<Value, String>) -> i32 {
    match r {
        Ok(v) => {
            println!("{}", v.to_json());
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn build_args(tool: &Tool, argv: &[String]) -> Result<Value, String> {
    let mut pairs: Vec<(String, Value)> = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let raw = argv[i].strip_prefix("--").ok_or_else(|| {
            format!(
                "unexpected argument \"{}\"; flags look like --name value",
                argv[i]
            )
        })?;
        // `--name=value` carries its value in the same word, so the value
        // may itself start with `--`.
        let (name, inline) = match raw.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (raw, None),
        };
        let flag = name.replace('-', "_");
        let Some(param) = tool.params.iter().find(|p| p.name == flag) else {
            let known: Vec<String> = tool
                .params
                .iter()
                .map(|p| format!("--{}", p.name.replace('_', "-")))
                .collect();
            return Err(format!(
                "unknown flag --{name} for {}; flags: {}",
                tool.name.trim_start_matches("adsertio_"),
                if known.is_empty() {
                    "none".to_string()
                } else {
                    known.join(", ")
                }
            ));
        };
        let kind = param.kind;
        if !matches!(kind, Kind::StrList) && pairs.iter().any(|(k, _)| *k == flag) {
            return Err(format!("--{name} given twice"));
        }
        // The value: inline after `=`, the word after `--` when the next
        // word is exactly `--`, or the next word when it is not a flag.
        let (value, used) = match inline {
            Some(v) => (Some(v), 1),
            None => match argv.get(i + 1).map(String::as_str) {
                Some("--") => match argv.get(i + 2) {
                    Some(v) => (Some(v.clone()), 3),
                    None => return Err(format!("--{name} needs a value after --")),
                },
                Some(v) if !v.starts_with("--") => (Some(v.to_string()), 2),
                _ => (None, 1),
            },
        };
        match (kind, value) {
            (Kind::Bool, Some(v)) if v == "true" || v == "false" => {
                pairs.push((flag, Value::Bool(v == "true")));
                i += used;
            }
            (Kind::Bool, Some(v)) if used != 2 => {
                return Err(format!("--{name} is true or false, got \"{v}\""));
            }
            (Kind::Bool, _) => {
                pairs.push((flag, Value::Bool(true)));
                i += 1;
            }
            (_, None) => return Err(format!("--{name} needs a value")),
            (Kind::UInt, Some(v)) => {
                let n: u64 = v
                    .parse()
                    .map_err(|_| format!("--{name} must be a whole number, got \"{v}\""))?;
                pairs.push((flag, Value::uint(n)));
                i += used;
            }
            (Kind::StrList, Some(v)) => {
                match pairs.iter_mut().find(|(k, _)| *k == flag) {
                    Some((_, Value::Array(items))) => items.push(Value::str(v.as_str())),
                    _ => pairs.push((flag, Value::Array(vec![Value::str(v.as_str())]))),
                }
                i += used;
            }
            (Kind::Str, Some(v)) => {
                pairs.push((flag, Value::str(v.as_str())));
                i += used;
            }
        }
    }
    Ok(Value::Object(pairs))
}
