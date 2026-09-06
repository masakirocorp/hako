//! Host observation/command job implementations (git, ports, process, commands).

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::execution_host::protocol::{
    CommandSpec, GitStatusSnapshot, ObservedProcess, PortSnapshot, PortTransport,
    ProcessObservation, ProjectCommandSnapshot, RuntimeExitStatus, WorkerError, WorkerErrorCode,
};
use crate::execution_host::{ExecutionHostId, HostPath, ResourceLocation};

use super::util::{
    worker_error, COMMAND_OUTPUT_BYTES, GIT_OUTPUT_BYTES, LSOF_OUTPUT_BYTES,
    MAX_COMMAND_DIRECTORY_ENTRIES, MAX_COMMAND_MANIFEST_BYTES,
};

#[cfg(unix)]
pub(super) fn git_status_at(
    cwd: &Path,
    cancel: Arc<AtomicBool>,
) -> Result<GitStatusSnapshot, WorkerError> {
    let mut command = Command::new("git");
    command.args([
        "-C",
        cwd.to_string_lossy().as_ref(),
        "status",
        "--porcelain=v2",
        "--branch",
    ]);
    let output = run_bounded_process(&mut command, GIT_OUTPUT_BYTES, cancel, "git status")?;
    if !output.status.success() {
        return Err(worker_error(
            WorkerErrorCode::Failed,
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut branch = None;
    let mut upstream = None;
    let mut ahead = 0;
    let mut behind = 0;
    let mut dirty = false;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("# branch.head ") {
            branch = (value != "(detached)").then(|| value.to_string());
        } else if let Some(value) = line.strip_prefix("# branch.upstream ") {
            upstream = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("# branch.ab ") {
            for part in value.split_whitespace() {
                if let Some(value) = part.strip_prefix('+') {
                    ahead = value.parse().unwrap_or(0);
                } else if let Some(value) = part.strip_prefix('-') {
                    behind = value.parse().unwrap_or(0);
                }
            }
        } else if !line.starts_with('#') {
            dirty = true;
        }
    }
    Ok(GitStatusSnapshot {
        branch,
        dirty,
        upstream,
        ahead,
        behind,
    })
}

#[cfg(unix)]
pub(super) fn list_worktrees_at(
    cwd: &Path,
    execution_host_id: &ExecutionHostId,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<crate::execution_host::protocol::WorktreeSnapshot>, WorkerError> {
    let mut command = Command::new("git");
    command.args([
        "-C",
        cwd.to_string_lossy().as_ref(),
        "worktree",
        "list",
        "--porcelain",
    ]);
    let output = run_bounded_process(&mut command, GIT_OUTPUT_BYTES, cancel, "git worktree list")?;
    if !output.status.success() {
        return Err(worker_error(
            WorkerErrorCode::Failed,
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    let mut result = Vec::new();
    let mut path = None::<String>;
    let mut branch = None::<String>;
    let mut bare = false;
    for line in String::from_utf8_lossy(&output.stdout)
        .lines()
        .chain(std::iter::once(""))
    {
        if line.is_empty() {
            if let Some(path) = path.take() {
                result.push(crate::execution_host::protocol::WorktreeSnapshot {
                    location: ResourceLocation::new(
                        execution_host_id.clone(),
                        HostPath::new(path).map_err(|err| {
                            worker_error(WorkerErrorCode::Failed, err.to_string())
                        })?,
                    ),
                    branch: branch.take(),
                    bare,
                });
            }
            bare = false;
        } else if let Some(value) = line.strip_prefix("worktree ") {
            path = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("branch ") {
            branch = Some(value.trim_start_matches("refs/heads/").to_string());
        } else if line == "bare" {
            bare = true;
        }
    }
    Ok(result)
}

#[cfg(unix)]
pub(super) fn observe_ports_at(
    execution_host_id: &ExecutionHostId,
    location: &ResourceLocation,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<PortSnapshot>, WorkerError> {
    if location.execution_host_id != *execution_host_id {
        return Err(worker_error(
            WorkerErrorCode::BindingMismatch,
            "resource location belongs to another execution host",
        ));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(worker_error(
            WorkerErrorCode::TimedOut,
            "port observation cancelled before start",
        ));
    }
    let mut command = Command::new("lsof");
    command.args(["-nP", "-iTCP", "-sTCP:LISTEN", "-F", "pcn"]);
    let output = run_bounded_process(&mut command, LSOF_OUTPUT_BYTES, cancel, "lsof")?;
    if !output.status.success() && output.stdout.is_empty() {
        return Err(worker_error(
            WorkerErrorCode::Failed,
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(crate::platform::parse_lsof_tcp_listeners(&text)
        .into_iter()
        .map(|listener| PortSnapshot {
            execution_host_id: execution_host_id.clone(),
            transport: PortTransport::Tcp,
            bind_address: listener.bind_addr.to_string(),
            port: listener.port,
            pid: Some(listener.pid),
            command: listener.command,
        })
        .collect())
}

#[cfg(unix)]
pub(super) fn observe_runtime_process(
    shell_pid: u32,
    location: &ResourceLocation,
) -> ProcessObservation {
    let shell_cwd = crate::platform::process_cwd(shell_pid)
        .and_then(|cwd| HostPath::new(cwd).ok())
        .or_else(|| Some(location.path.clone()));
    let shell_command = crate::platform::foreground_job(shell_pid)
        .and_then(|job| {
            job.processes
                .into_iter()
                .find(|process| process.pid == shell_pid)
                .map(|process| process.name)
        })
        .or_else(|| {
            // Fall back to session membership when the shell is idle and not
            // reported as its own foreground job member.
            observed_process(shell_pid).map(|process| process.name)
        });

    let foreground_job = (shell_pid != 0)
        .then(|| crate::platform::foreground_job(shell_pid))
        .flatten();
    let foreground_process_group_id = foreground_job.as_ref().map(|job| job.process_group_id);
    let mut foreground_processes = foreground_job
        .map(|job| {
            job.processes
                .into_iter()
                .filter_map(observed_process_from_foreground)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    // Idle shells own the tty foreground group; API consumers expect an empty
    // foreground list rather than reporting the shell child as busy work.
    if !foreground_processes.is_empty()
        && foreground_processes
            .iter()
            .all(|process| process.pid == shell_pid)
    {
        foreground_processes.clear();
    }

    let mut session_pids = if shell_pid == 0 {
        Vec::new()
    } else {
        crate::platform::session_processes(shell_pid)
    };
    if shell_pid != 0 && !session_pids.contains(&shell_pid) {
        session_pids.push(shell_pid);
    }
    let session_processes = session_pids
        .into_iter()
        .filter_map(observed_process)
        .collect::<Vec<_>>();

    ProcessObservation {
        pid: shell_pid,
        ppid: None,
        command: shell_command,
        cwd: shell_cwd,
        foreground_process_group_id,
        foreground_processes,
        session_processes,
    }
}

#[cfg(unix)]
pub(super) fn observed_process(pid: u32) -> Option<ObservedProcess> {
    if pid == 0 {
        return None;
    }
    let job = crate::platform::foreground_job(pid);
    if let Some(process) =
        job.and_then(|job| job.processes.into_iter().find(|process| process.pid == pid))
    {
        return observed_process_from_foreground(process);
    }
    // session_processes may include descendants that are not in a detectable
    // foreground job; synthesize a minimal observation from cwd alone.
    Some(ObservedProcess {
        pid,
        name: format!("pid-{pid}"),
        argv0: None,
        argv: None,
        cmdline: None,
        cwd: crate::platform::process_cwd(pid).and_then(|cwd| HostPath::new(cwd).ok()),
    })
}

#[cfg(unix)]
pub(super) fn observed_process_from_foreground(
    process: crate::platform::ForegroundProcess,
) -> Option<ObservedProcess> {
    if process.pid == 0 {
        return None;
    }
    Some(ObservedProcess {
        pid: process.pid,
        name: process.name,
        argv0: process.argv0,
        argv: process.argv,
        cmdline: process.cmdline,
        cwd: crate::platform::process_cwd(process.pid).and_then(|cwd| HostPath::new(cwd).ok()),
    })
}

#[cfg(unix)]
pub(super) fn discover_project_commands_at(
    execution_host_id: &ExecutionHostId,
    location: &ResourceLocation,
    cancel: Arc<AtomicBool>,
) -> Result<(ResourceLocation, Vec<ProjectCommandSnapshot>), WorkerError> {
    if location.execution_host_id != *execution_host_id {
        return Err(worker_error(
            WorkerErrorCode::BindingMismatch,
            "resource location belongs to another execution host",
        ));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(worker_error(
            WorkerErrorCode::TimedOut,
            "project command discovery cancelled before start",
        ));
    }
    let cwd = location.path.as_path();
    let root = crate::commands::project_root_from_cwd(cwd);
    if cancel.load(Ordering::Relaxed) {
        return Err(worker_error(
            WorkerErrorCode::TimedOut,
            "project command discovery cancelled",
        ));
    }
    let root_path = HostPath::new(root.clone())
        .map_err(|error| worker_error(WorkerErrorCode::InvalidLocation, error.to_string()))?;
    let resolved = ResourceLocation::new(execution_host_id.clone(), root_path);
    // Bound common manifests before full discovery so oversized files fail typed.
    for name in [
        "package.json",
        "composer.json",
        "justfile",
        "Justfile",
        "Makefile",
        ".vscode/tasks.json",
    ] {
        let path = root.join(name);
        if let Ok(meta) = std::fs::metadata(&path) {
            let bytes = meta.len() as usize;
            if bytes > MAX_COMMAND_MANIFEST_BYTES {
                return Err(worker_error(
                    WorkerErrorCode::OutputTooLarge,
                    format!(
                        "command manifest {} is {bytes} bytes (max {MAX_COMMAND_MANIFEST_BYTES})",
                        path.display()
                    ),
                ));
            }
        }
    }
    if let Ok(read_dir) = std::fs::read_dir(&root) {
        let count = read_dir.count();
        if count > MAX_COMMAND_DIRECTORY_ENTRIES {
            return Err(worker_error(
                WorkerErrorCode::OutputTooLarge,
                format!(
                    "command directory {} has {count} entries (max {MAX_COMMAND_DIRECTORY_ENTRIES})",
                    root.display()
                ),
            ));
        }
    }
    let commands = crate::commands::discover_project_commands_at(&resolved);
    Ok((
        resolved,
        commands
            .iter()
            .map(crate::commands::project_command_to_snapshot)
            .collect(),
    ))
}

#[cfg(unix)]
pub(super) struct BoundedProcessOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

pub(super) fn read_bounded_command_output(
    mut reader: impl Read,
    max_bytes: usize,
    cancel: &AtomicBool,
) -> io::Result<(Vec<u8>, bool)> {
    let mut output = Vec::new();
    let mut oversized = false;
    let mut buffer = [0_u8; 8192];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok((output, oversized));
        }
        let read = match reader.read(&mut buffer) {
            Ok(0) => return Ok((output, oversized)),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        let remaining = max_bytes.saturating_sub(output.len());
        let retained = remaining.min(read);
        output.extend_from_slice(&buffer[..retained]);
        oversized |= retained != read;
        if oversized {
            // Drain remaining input without retaining it so the child can exit.
            let mut discard = [0_u8; 8192];
            loop {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                match reader.read(&mut discard) {
                    Ok(0) => break,
                    Ok(_) => continue,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error),
                }
            }
            return Ok((output, true));
        }
    }
}

#[cfg(unix)]
pub(super) fn run_bounded_process(
    command: &mut Command,
    max_bytes: usize,
    cancel: Arc<AtomicBool>,
    label: &str,
) -> Result<BoundedProcessOutput, WorkerError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(worker_error(
            WorkerErrorCode::TimedOut,
            format!("{label} cancelled before start"),
        ));
    }
    crate::platform::configure_cancellable_command(command);
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| worker_error(WorkerErrorCode::Failed, err.to_string()))?;
    let stdout = child.stdout.take().ok_or_else(|| {
        worker_error(
            WorkerErrorCode::Failed,
            format!("{label} stdout pipe is unavailable"),
        )
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        worker_error(
            WorkerErrorCode::Failed,
            format!("{label} stderr pipe is unavailable"),
        )
    })?;
    let stdout_cancel = cancel.clone();
    let stderr_cancel = cancel.clone();
    let stdout_reader = std::thread::spawn(move || {
        read_bounded_command_output(stdout, max_bytes, stdout_cancel.as_ref())
    });
    let stderr_reader = std::thread::spawn(move || {
        read_bounded_command_output(stderr, max_bytes, stderr_cancel.as_ref())
    });

    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            crate::platform::terminate_cancellable_child(&mut child);
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(worker_error(
                WorkerErrorCode::TimedOut,
                format!("{label} exceeded host job time limit"),
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(err) => {
                crate::platform::terminate_cancellable_child(&mut child);
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(worker_error(WorkerErrorCode::Failed, err.to_string()));
            }
        }
    };

    let (stdout, stdout_oversized) = stdout_reader
        .join()
        .map_err(|_| {
            worker_error(
                WorkerErrorCode::Failed,
                format!("{label} stdout reader panicked"),
            )
        })?
        .map_err(|err| worker_error(WorkerErrorCode::Failed, err.to_string()))?;
    let (stderr, stderr_oversized) = stderr_reader
        .join()
        .map_err(|_| {
            worker_error(
                WorkerErrorCode::Failed,
                format!("{label} stderr reader panicked"),
            )
        })?
        .map_err(|err| worker_error(WorkerErrorCode::Failed, err.to_string()))?;
    if stdout_oversized || stderr_oversized {
        return Err(worker_error(
            WorkerErrorCode::OutputTooLarge,
            format!("{label} output exceeded {max_bytes} bytes per stream"),
        ));
    }
    Ok(BoundedProcessOutput {
        status,
        stdout,
        stderr,
    })
}

#[cfg(unix)]
pub(super) fn run_command_at(
    cwd: PathBuf,
    command: CommandSpec,
    cancel: Arc<AtomicBool>,
) -> Result<(RuntimeExitStatus, Vec<u8>, Vec<u8>), WorkerError> {
    command
        .validate()
        .map_err(|err| worker_error(WorkerErrorCode::Failed, err.to_string()))?;
    let mut process = Command::new(&command.program);
    process
        .args(&command.args)
        .envs(command.env.iter().cloned())
        .current_dir(cwd);
    let output = run_bounded_process(&mut process, COMMAND_OUTPUT_BYTES, cancel, "command")?;
    use std::os::unix::process::ExitStatusExt as _;
    let exit = output
        .status
        .code()
        .map(RuntimeExitStatus::Code)
        .or_else(|| output.status.signal().map(RuntimeExitStatus::Signal))
        .unwrap_or(RuntimeExitStatus::Code(1));
    Ok((exit, output.stdout, output.stderr))
}
