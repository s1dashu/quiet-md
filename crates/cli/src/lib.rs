//! `quiet_cli` (`qmd`) — Fine-grained, GPUI-free, agent-friendly CLI for Quiet.md.

pub mod args;
pub mod assets;
pub mod board_builder;
pub mod commands;
pub mod lint;
pub mod locator;
pub mod output;
pub mod spec;

use args::{ParseOutcome, ParsedCmd, known_top_command};
use output::{GlobalOpts, print_error};

/// Returns `true` when `argv` (including program name at index 0) should be
/// handled by the CLI instead of launching the desktop GUI / opening `.md` files.
pub fn should_intercept_args(argv: &[String]) -> bool {
    let Some(first) = argv.get(1) else { return false; };
    if matches!(
        first.as_str(),
        "-h" | "--help" | "-V" | "--version" | "--json" | "--dry-run" | "--quiet" | "-q" | "--lib"
    ) || first.starts_with("--lib=")
    {
        return true;
    }
    !first.contains('/') && !first.contains('\\') && known_top_command(first)
}

/// If invoked with CLI arguments from `quiet_md`, executes the CLI and returns `Some(exit_code)`.
/// Returns `None` when the desktop GUI should start normally.
pub fn maybe_run_from_args() -> Option<i32> {
    let argv: Vec<String> = std::env::args().collect();
    if !should_intercept_args(&argv) {
        return None;
    }
    Some(run_args(&argv[1..]))
}

pub fn run_from_env() -> i32 {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    run_args(&argv)
}

pub fn run_args(args: &[String]) -> i32 {
    match args::parse(args) {
        Ok((opts, ParseOutcome::Help(text))) => {
            if opts.json {
                output::print_success(
                    &opts,
                    "help",
                    opts.lib.as_deref(),
                    serde_json::json!({ "help": text }),
                    &[],
                    &[],
                    &text,
                );
            } else {
                println!("{text}");
            }
            0
        }
        Ok((opts, ParseOutcome::Version(ver))) => {
            if opts.json {
                output::print_success(
                    &opts,
                    "version",
                    opts.lib.as_deref(),
                    serde_json::json!({ "version": env!("CARGO_PKG_VERSION"), "banner": ver }),
                    &[],
                    &[],
                    &ver,
                );
            } else {
                println!("{ver}");
            }
            0
        }
        Ok((opts, ParseOutcome::Run(cmd))) => {
            let cmd_path = cmd.command_path.clone();
            match dispatch(&cmd, &opts) {
                Ok(()) => 0,
                Err(fail) => {
                    let lib = opts.lib.clone().or_else(|| Some(commands::resolve_library_root(&opts)));
                    print_error(&opts, &cmd_path, lib.as_deref(), &fail);
                    fail.exit_code
                }
            }
        }
        Err((opts, fail)) => {
            let lib = opts.lib.as_deref();
            print_error(&opts, "parse", lib, &fail);
            fail.exit_code
        }
    }
}

fn dispatch(cmd: &ParsedCmd, opts: &GlobalOpts) -> Result<(), output::CliFailure> {
    let group = cmd.command_path.split_whitespace().next().unwrap_or("");
    match group {
        "schema" => commands::schema::run(cmd, opts),
        "library" => commands::library::run(cmd, opts),
        "folder" => commands::folder::run(cmd, opts),
        "doc" => commands::doc::run(cmd, opts),
        "meta" => commands::meta::run(cmd, opts),
        "block" => commands::block::run(cmd, opts),
        "table" => commands::table::run(cmd, opts),
        "board" => commands::board::run(cmd, opts),
        "asset" => commands::asset::run(cmd, opts),
        "theme" => commands::theme::run(cmd, opts),
        "validate" => commands::validate::run(cmd, opts),
        "export" => commands::export::run(cmd, opts),
        other => Err(output::CliFailure::usage(format!("Unknown command group '{other}'."))),
    }
}
