use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::Command;
use std::thread::JoinHandle;

const STDERR_LINES: usize = 20;

/// A `Command` that does not open a console window on Windows.
pub fn command(program: &Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Drains a child's stderr on a thread (so the pipe never fills up) and keeps the last lines for
/// error messages.
pub struct StderrTail(Option<JoinHandle<String>>);

impl StderrTail {
    pub fn spawn(stderr: impl Read + Send + 'static) -> Self {
        Self(Some(std::thread::spawn(move || {
            let mut tail = VecDeque::with_capacity(STDERR_LINES);
            for line in BufReader::new(stderr).split(b'\n').map_while(Result::ok) {
                if tail.len() == STDERR_LINES {
                    tail.pop_front();
                }
                tail.push_back(String::from_utf8_lossy(&line).trim_end().to_owned());
            }
            Vec::from(tail).join("\n")
        })))
    }

    /// Waits for the stream to close and returns the collected tail.
    pub fn finish(&mut self) -> String {
        self.0
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default()
    }
}
