//! The command line: `adsertio <command> --flag value ...`.
//!
//! Commands are the MCP tools without the `adsertio_` prefix (claim, view), plus
//! init, check, mcp. Flags come from the same parameter table as
//! the MCP schema. An unknown flag, a flag with no value, or a bad number is
//! an error; nothing is guessed.

use crate::json::Value;
use crate::tool::{Kind, Tool};
use crate::{claim, store::Store, tools, SERVER, VERSION};

const HELP: &str = "adsertio: one verified-or-unverified view per claim

USAGE
  adsertio init                       create .adsertio/ in this folder
  adsertio claim --summary S [--evidence tool:path[#selector]]... [--author A] [--body B] [--id I] [--tags T]...
  adsertio attach --id I --evidence tool:path[#selector]...
  adsertio view [--id I] [--verdict verified|refuted|unverified] [--states S]... [--compact] [--limit N] [--offset N]
  adsertio ls [--limit N] [--offset N]
  adsertio guide
  adsertio check                      read every claim; fail on any damage
  adsertio mcp                        start the stdio MCP server (same as adsertio-mcp)
  adsertio --version

Evidence tools: trammel, chisel, probatio. Output is JSON; the exit code is 1 on
any error, 2 on a usage error.";

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
        "check" => finish(check()),
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
        let flag = argv[i].strip_prefix("--").ok_or_else(|| {
            format!(
                "unexpected argument \"{}\"; flags look like --name value",
                argv[i]
            )
        })?;
        let flag = flag.replace('-', "_");
        let param = tool.params.iter().find(|p| p.name == flag);
        let kind = param.map_or(Kind::Str, |p| p.kind);
        let value = argv.get(i + 1).filter(|v| !v.starts_with("--"));
        match (kind, value) {
            (Kind::Bool, Some(v)) if v == "true" || v == "false" => {
                pairs.push((flag, Value::Bool(v == "true")));
                i += 2;
            }
            (Kind::Bool, _) => {
                pairs.push((flag, Value::Bool(true)));
                i += 1;
            }
            (_, None) => {
                return Err(format!(
                    "--{} needs a value",
                    argv[i].trim_start_matches("--")
                ))
            }
            (Kind::UInt, Some(v)) => {
                let n: u64 = v.parse().map_err(|_| {
                    format!(
                        "--{} must be a whole number, got \"{v}\"",
                        argv[i].trim_start_matches("--")
                    )
                })?;
                pairs.push((flag, Value::uint(n)));
                i += 2;
            }
            (Kind::StrList, Some(v)) => {
                match pairs.iter_mut().find(|(k, _)| *k == flag) {
                    Some((_, Value::Array(items))) => items.push(Value::str(v.as_str())),
                    _ => pairs.push((flag, Value::Array(vec![Value::str(v.as_str())]))),
                }
                i += 2;
            }
            (Kind::Str, Some(v)) => {
                if pairs.iter().any(|(k, _)| *k == flag) {
                    return Err(format!(
                        "--{} given twice",
                        argv[i].trim_start_matches("--")
                    ));
                }
                pairs.push((flag, Value::str(v.as_str())));
                i += 2;
            }
        }
    }
    Ok(Value::Object(pairs))
}

/// Read every claim. Any damage is an error; stray temp files are reported so
/// they can be removed.
fn check() -> Result<Value, String> {
    let store = Store::open()?;
    let claims = claim::load_all(&store)?;
    let mut strays = Vec::new();
    if let Ok(rd) = std::fs::read_dir(store.claims_dir()) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with('.') && n.contains(".tmp-") {
                strays.push(Value::String(n));
            }
        }
    }
    Ok(Value::obj(vec![
        ("ok", Value::Bool(true)),
        ("claims", Value::uint(claims.len() as u64)),
        ("stray_temp_files", Value::Array(strays)),
    ]))
}
