//! The command line: `adsertio <command> --flag value ...`.
//!
//! Commands are the MCP tools without the `adsertio_` prefix (claim, view), plus
//! init, check, mcp. Flags come from the same parameter table as
//! the MCP schema. An unknown flag, a repeated flag, a flag with no value, or
//! a bad number is a usage error (exit 2); nothing is guessed. A value that
//! starts with `--` is passed as `--name=value` or `--name -- value`.

use crate::json::Value;
use crate::tool::{Kind, Param, Tool};
use crate::{store::Store, tools, SERVER, VERSION};

const HELP: &str = r#"adsertio: one verified-or-unverified view per claim

USAGE
  adsertio init                       create .adsertio/ in this folder
  adsertio claim --claims JSON | --claims-file PATH    several claims at once (preferred)
  adsertio claim --summary S [--evidence tool:path[#selector]]... [--author A] [--body B] [--id I] [--tags T]...
  adsertio attach --items JSON | --items-file PATH     several claims at once
  adsertio attach --id I --evidence tool:path[#selector]...
  adsertio view [--id I] [--verdict verified|refuted|unverified] [--states S]... [--compact] [--limit N] [--offset N] [--after CURSOR] [--wrap cursor|claude-subagent]
  adsertio ls [--limit N] [--offset N]
  adsertio guide
  adsertio check                      read every claim; fail on any damage
  adsertio doctor [--fix]             diagnose the store, git and the evidence stores
  adsertio hook [--host claude|cursor|generic]    print a session-start hook to paste
  adsertio mcp                        start the stdio MCP server (same as adsertio-mcp)
  adsertio --version

A value that starts with -- goes after =, as in --summary=--fast, or after a
lone --, as in --summary -- --fast. Each flag is given once, except --evidence,
--tags and --states, which repeat.

--claims is a JSON array of up to 50 objects, each with the fields of one
claim: [{"summary":"...","evidence":["chisel:.chisel/store#test:t1"]}].
--items is a JSON array of {"id":"...","evidence":[...]}. On Windows, where
quoting JSON differs between cmd, PowerShell and Git Bash, write the array to
a file and pass --claims-file or --items-file.

Evidence tools: trammel, chisel, probatio. Output is JSON; the exit code is 1 on
any error, 2 on a usage error (an unknown command or flag, a repeated flag, a
missing or bad value). doctor exits 1 when a check fails."#;

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
        "hook" => match crate::hook::from_argv(&args[1..]) {
            Ok(text) => {
                print!("{text}");
                0
            }
            Err(e) => {
                eprintln!("{e}");
                2
            }
        },
        "view" => view(&args[1..]),
        "doctor" => doctor(&args[1..]),
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

fn tool(name: &str) -> &'static Tool {
    tools::TOOLS
        .iter()
        .find(|t| t.name == name)
        .expect("the tool is in the table")
}

fn run_tool(tool: &Tool, argv: &[String]) -> Result<Result<Value, String>, i32> {
    match build_args(tool, argv) {
        Err(e) => {
            eprintln!("{e}");
            Err(2)
        }
        Ok(v) => Ok(tool.validate(&v).and_then(|a| (tool.run)(&a))),
    }
}

/// `view`, plus the command-line-only `--wrap cursor|claude-subagent`, which
/// prints the view inside the JSON a host hook reads.
fn view(argv: &[String]) -> i32 {
    let mut rest = Vec::new();
    let mut wrap: Option<String> = None;
    let mut i = 0;
    while i < argv.len() {
        let word = &argv[i];
        let value = if word == "--wrap" {
            i += 1;
            match argv.get(i) {
                Some(v) if !v.starts_with("--") => Some(v.clone()),
                _ => {
                    eprintln!("--wrap needs a value: {}", crate::hook::WRAPS.join(" or "));
                    return 2;
                }
            }
        } else {
            word.strip_prefix("--wrap=").map(str::to_string)
        };
        match value {
            Some(v) if wrap.is_some() => {
                eprintln!("--wrap given twice (second: {v})");
                return 2;
            }
            Some(v) if !crate::hook::WRAPS.contains(&v.as_str()) => {
                eprintln!(
                    "--wrap must be one of: {}, not \"{v}\"",
                    crate::hook::WRAPS.join(", ")
                );
                return 2;
            }
            Some(v) => wrap = Some(v),
            None => rest.push(word.clone()),
        }
        i += 1;
    }
    let out = match run_tool(tool("adsertio_view"), &rest) {
        Ok(out) => out,
        Err(code) => return code,
    };
    match (wrap, out) {
        (Some(host), Ok(v)) => match crate::hook::wrap(&host, &v.to_json()) {
            Ok(text) => {
                println!("{text}");
                0
            }
            Err(e) => finish(Err(e)),
        },
        (_, out) => finish(out),
    }
}

/// `doctor` prints its JSON report either way and exits 1 when a check failed.
fn doctor(argv: &[String]) -> i32 {
    match run_tool(tool("adsertio_doctor"), argv) {
        Err(code) => code,
        Ok(Ok(v)) => {
            println!("{}", v.to_json());
            if v.get("ok").and_then(Value::as_bool) == Some(true) {
                0
            } else {
                1
            }
        }
        Ok(Err(e)) => finish(Err(e)),
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
    parse_flags(tool.params, tool.name.trim_start_matches("adsertio_"), argv)
}

/// Longest JSON file `--<name>-file` reads, the same as one MCP message.
const MAX_FLAG_FILE: u64 = crate::mcp::MAX_LINE as u64;

/// Parse `--name value` flags against a parameter table. The rules every
/// command shares: an unknown flag, a repeated flag (other than a list), a
/// missing value, or a bad number is an error. A list-of-objects parameter
/// takes JSON text (`--claims '[...]'`) or a JSON file (`--claims-file PATH`).
pub fn parse_flags(params: &[Param], command: &str, argv: &[String]) -> Result<Value, String> {
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
        let mut flag = name.replace('-', "_");
        let mut from_file = false;
        if !params.iter().any(|p| p.name == flag) {
            if let Some(base) = flag.strip_suffix("_file") {
                if params
                    .iter()
                    .any(|p| p.name == base && p.kind == Kind::ObjList)
                {
                    flag = base.to_string();
                    from_file = true;
                }
            }
        }
        let Some(param) = params.iter().find(|p| p.name == flag) else {
            let mut known: Vec<String> = Vec::new();
            for p in params {
                known.push(format!("--{}", p.name.replace('_', "-")));
                if p.kind == Kind::ObjList {
                    known.push(format!("--{}-file", p.name.replace('_', "-")));
                }
            }
            return Err(format!(
                "unknown flag --{name} for {command}; flags: {}",
                if known.is_empty() {
                    "none".to_string()
                } else {
                    known.join(", ")
                }
            ));
        };
        let kind = param.kind;
        if !matches!(kind, Kind::StrList) && pairs.iter().any(|(k, _)| *k == flag) {
            if kind == Kind::ObjList {
                let dashed = flag.replace('_', "-");
                return Err(format!(
                    "--{dashed} or --{dashed}-file, once: they were given together or twice"
                ));
            }
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
            (Kind::ObjList, Some(v)) => {
                let text = if from_file {
                    read_flag_file(name, &v)?
                } else {
                    v
                };
                let parsed = crate::json::parse(&text)
                    .map_err(|e| format!("--{name} is not valid JSON: {e}"))?;
                if !matches!(parsed, Value::Array(_)) {
                    return Err(format!(
                        "--{name} must be a JSON array of objects, got {}",
                        parsed.type_name()
                    ));
                }
                pairs.push((flag, parsed));
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

fn read_flag_file(flag: &str, path: &str) -> Result<String, String> {
    let path = std::path::Path::new(path);
    let meta = std::fs::metadata(path)
        .map_err(|e| format!("--{flag}: cannot read {}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("--{flag}: {} is not a file", path.display()));
    }
    if meta.len() > MAX_FLAG_FILE {
        return Err(format!(
            "--{flag}: {} is larger than {MAX_FLAG_FILE} bytes",
            path.display()
        ));
    }
    let text = crate::fsx::read_strict(path).map_err(|e| format!("--{flag}: {e}"))?;
    // Windows PowerShell 5 writes a byte order mark at the start of a UTF-8 file.
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_string())
}
