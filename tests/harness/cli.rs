use super::{
    lifecycle::{LifecycleOptions, run_lifecycle},
    runtime::hook_main,
    runtime_driver::{RuntimeOptions, run_runtime},
};
use std::{
    collections::BTreeMap,
    env,
    io::{self, Write},
    path::PathBuf,
};

struct Flag {
    value: String,
    description: &'static str,
    boolean: bool,
}
fn flags(runtime: bool) -> BTreeMap<&'static str, Flag> {
    let mut flags = BTreeMap::new();
    for (name, value, description) in [
        (
            "source",
            ".",
            if runtime {
                "source checkout for the evidence manifest"
            } else {
                "source checkout; faults run only in an external copy"
            },
        ),
        ("output", "", "new evidence directory outside Git checkouts"),
    ] {
        flags.insert(
            name,
            Flag {
                value: value.into(),
                description,
                boolean: false,
            },
        );
    }
    if runtime {
        for (name, value, description) in [
            (
                "hook",
                "",
                "run the synthetic hook adapter for claude, pi, or codex",
            ),
            ("entry", "", "absolute assembled bin/agent-guard path"),
            (
                "runtimes",
                "claude,pi,codex",
                "comma-separated runtimes; each runs all ten cases three times",
            ),
        ] {
            flags.insert(
                name,
                Flag {
                    value: value.into(),
                    description,
                    boolean: false,
                },
            );
        }
        flags.insert(
            "ablate",
            Flag {
                value: "false".into(),
                description: "synthetic no-guard control; every case must execute",
                boolean: true,
            },
        );
    } else {
        for (name, value, description) in [
            (
                "cargo",
                "cargo",
                "Cargo executable for copied Rust fault builds",
            ),
            (
                "control",
                "",
                "negative control: drain, stderr-drain, failclosed, deadline, cleanup, dependency",
            ),
        ] {
            flags.insert(
                name,
                Flag {
                    value: value.into(),
                    description,
                    boolean: false,
                },
            );
        }
    }
    flags
}
fn usage(program: &str, flags: &BTreeMap<&str, Flag>, stderr: &mut impl Write) -> io::Result<()> {
    writeln!(stderr, "Usage of {program}:")?;
    for (name, flag) in flags {
        if flag.boolean {
            writeln!(stderr, "  -{name}\n    \t{}", flag.description)?;
        } else {
            write!(stderr, "  -{name} string\n    \t{}", flag.description)?;
            if !flag.value.is_empty() {
                write!(stderr, " (default {:?})", flag.value)?;
            }
            writeln!(stderr)?;
        }
    }
    Ok(())
}
pub fn main(runtime: bool) -> i32 {
    let args: Vec<_> = env::args().collect();
    let mut definitions = flags(runtime);
    let defaults = flags(runtime);
    let mut index = 1;
    let mut stderr = io::stderr().lock();
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" || !arg.starts_with('-') || arg == "-" {
            break;
        }
        let stripped = arg
            .strip_prefix("--")
            .or_else(|| arg.strip_prefix('-'))
            .unwrap_or(arg);
        let (name, value) = stripped
            .split_once('=')
            .map_or((stripped, None), |(name, value)| (name, Some(value)));
        if matches!(name, "h" | "help") {
            let _ = usage(&args[0], &defaults, &mut stderr);
            return 0;
        }
        let Some(flag) = definitions.get_mut(name) else {
            let _ = writeln!(stderr, "flag provided but not defined: -{name}");
            let _ = usage(&args[0], &defaults, &mut stderr);
            return 2;
        };
        if flag.boolean {
            flag.value = match value.unwrap_or("true") {
                "1" | "t" | "T" | "TRUE" | "true" | "True" => "true".into(),
                "0" | "f" | "F" | "FALSE" | "false" | "False" => "false".into(),
                bad => {
                    let _ = writeln!(
                        stderr,
                        "invalid boolean value {bad:?} for -{name}: parse error"
                    );
                    let _ = usage(&args[0], &defaults, &mut stderr);
                    return 2;
                }
            };
        } else if let Some(value) = value {
            flag.value = value.into();
        } else {
            index += 1;
            if index >= args.len() {
                let _ = writeln!(stderr, "flag needs an argument: -{name}");
                let _ = usage(&args[0], &defaults, &mut stderr);
                return 2;
            }
            flag.value = args[index].clone();
        }
        index += 1;
    }
    let value = |name| {
        definitions
            .get(name)
            .map(|flag| flag.value.clone())
            .unwrap_or_default()
    };
    if runtime && !value("hook").is_empty() {
        return hook_main(
            &value("hook"),
            &mut io::stdin().lock(),
            &mut io::stdout().lock(),
            &mut stderr,
        );
    }
    let result = if runtime {
        env::current_exe().map_err(Into::into).and_then(|helper| {
            run_runtime(&RuntimeOptions {
                source: PathBuf::from(value("source")),
                output: PathBuf::from(value("output")),
                entry: PathBuf::from(value("entry")),
                helper,
                runtimes: value("runtimes").split(',').map(String::from).collect(),
                ablate: value("ablate") == "true",
            })
        })
    } else {
        run_lifecycle(&LifecycleOptions {
            source: PathBuf::from(value("source")),
            output: PathBuf::from(value("output")),
            cargo: value("cargo"),
            control: value("control"),
        })
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            let _ = writeln!(stderr, "{e}");
            1
        }
    }
}
