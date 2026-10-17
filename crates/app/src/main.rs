// The desktop app must not open a console window on Windows; CLI commands attach to the console
// of the terminal they were started from instead.
#![cfg_attr(all(windows, feature = "desktop"), windows_subsystem = "windows")]

mod cli;
#[cfg(feature = "desktop")]
mod desktop;
mod serve;
mod timespec;

use std::process::ExitCode;

use clap::Parser;

/// GUI-subsystem programs start without a console: attach to the terminal's console so CLI output
/// is visible, unless the output is already redirected (pipes, files).
#[cfg(all(windows, feature = "desktop"))]
fn attach_console() {
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
    };
    // SAFETY: plain Win32 calls without pointer arguments; failures are harmless.
    unsafe {
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        if out.is_null() || out == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

fn main() -> ExitCode {
    #[cfg(all(windows, feature = "desktop"))]
    if std::env::args().len() > 1 {
        attach_console();
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let cli = cli::Cli::parse();
    let is_command = cli.is_command();
    match cli.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            if !is_command {
                tracing::error!("{e:#}");
            }
            ExitCode::FAILURE
        }
    }
}
