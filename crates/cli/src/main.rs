//! `dm` executable. See `dm --help` and crates/cli/src/lib.rs.

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => {
            eprintln!("dm: error: cannot start async runtime");
            return ExitCode::from(dm_cli::exit::UNEXPECTED);
        }
    };
    let out = runtime.block_on(dm_cli::run_cli(&args, &dm_cli::RunOptions::default()));
    // Write errors (e.g. a closed pipe) must not turn into a panic.
    let _ = std::io::stdout().write_all(out.stdout.as_bytes());
    let _ = std::io::stderr().write_all(out.stderr.as_bytes());
    ExitCode::from(out.exit)
}
