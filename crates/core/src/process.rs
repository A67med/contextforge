//! Running child processes with a hard timeout and without flashing console windows on Windows.

use std::io::{Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Output {
    pub fn success(&self) -> bool {
        self.status.success()
    }
    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
    pub fn stderr_str(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// Prevent console windows from appearing when a GUI process spawns console programs.
pub fn hide_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Run `cmd` to completion, killing it if it exceeds `timeout`. Output is capped at `max_output` bytes per stream.
pub fn run(mut cmd: Command, stdin: Option<&[u8]>, timeout: Duration, max_output: usize) -> Result<Output> {
    hide_window(&mut cmd);
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let program = format!("{:?}", cmd.get_program());
    let mut child = cmd.spawn().map_err(|e| Error::Io(std::io::Error::new(e.kind(), format!("failed to start {program}: {e}"))))?;

    if let Some(input) = stdin {
        if let Some(mut pipe) = child.stdin.take() {
            let data = input.to_vec();
            // Write on a thread so a child that does not read stdin cannot deadlock us.
            std::thread::spawn(move || {
                let _ = pipe.write_all(&data);
            });
        }
    }

    let out_handle = spawn_reader(child.stdout.take(), max_output);
    let err_handle = spawn_reader(child.stderr.take(), max_output);
    let status = wait_with_timeout(&mut child, timeout).map_err(|e| Error::Io(std::io::Error::new(e.kind(), format!("{program}: {e}"))))?;
    let stdout = out_handle.join().unwrap_or_default();
    let stderr = err_handle.join().unwrap_or_default();
    Ok(Output { status, stdout, stderr })
}

fn spawn_reader<R: Read + Send + 'static>(pipe: Option<R>, max: usize) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = pipe {
            let mut chunk = [0u8; 8192];
            loop {
                match p.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if buf.len() < max {
                            let take = n.min(max - buf.len());
                            buf.extend_from_slice(&chunk[..take]);
                        }
                    }
                }
            }
        }
        buf
    })
}

fn wait_with_timeout(child: &mut Child, timeout: Duration) -> std::io::Result<ExitStatus> {
    let start = Instant::now();
    let mut sleep = Duration::from_millis(1);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, format!("timed out after {}ms", timeout.as_millis())));
        }
        std::thread::sleep(sleep);
        sleep = (sleep * 2).min(Duration::from_millis(20));
    }
}

/// Locate an executable on PATH (honours PATHEXT on Windows).
pub fn which(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| e.to_ascii_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        if cfg!(windows) {
            for ext in &exts {
                let cand = dir.join(format!("{name}{ext}"));
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
        let cand = dir.join(name);
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_kills_long_running_process() {
        let cmd = if cfg!(windows) {
            let mut c = Command::new("powershell");
            c.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 10"]);
            c
        } else {
            let mut c = Command::new("sleep");
            c.arg("10");
            c
        };
        let start = Instant::now();
        let r = run(cmd, None, Duration::from_millis(300), 1024);
        assert!(r.is_err());
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn captures_stdout() {
        let mut cmd = Command::new("git");
        cmd.arg("--version");
        let out = run(cmd, None, Duration::from_secs(10), 4096).unwrap();
        assert!(out.success());
        assert!(out.stdout_str().starts_with("git version"));
    }
}
