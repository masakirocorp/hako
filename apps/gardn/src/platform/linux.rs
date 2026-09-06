use std::{
    collections::HashMap,
    io::Write,
    os::fd::RawFd,
    path::PathBuf,
    process::{Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

use super::{
    read_limited_reader, ClipboardCommand, ClipboardImage, ForegroundJob, ForegroundProcess,
    LimitedRead, Signal,
};

const FOREGROUND_MEMBERS_CACHE_TTL: Duration = Duration::from_millis(250);

const WSL_MARKER_ENV_VARS: &[&str] = &["WSL_DISTRO_NAME", "WSL_INTEROP"];

pub(crate) fn should_draw_host_cursor_by_default_platform() -> bool {
    running_inside_wsl()
}

fn running_inside_wsl() -> bool {
    proc_file_indicates_wsl("/proc/sys/kernel/osrelease")
        || proc_file_indicates_wsl("/proc/version")
        || WSL_MARKER_ENV_VARS
            .iter()
            .any(|key| std::env::var_os(key).is_some())
        || std::path::Path::new("/run/WSL").exists()
}

fn proc_file_indicates_wsl(path: &str) -> bool {
    std::fs::read_to_string(path)
        .map(|text| text_indicates_wsl(&text))
        .unwrap_or(false)
}

fn text_indicates_wsl(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("microsoft") || text.contains("wsl")
}

pub(crate) fn scrollback_editor_argv_platform(
    path: &std::path::Path,
) -> std::io::Result<Vec<String>> {
    let quoted_path = shell_quote(&path.display().to_string());
    let command = format!(
        r#"scrollback_file={quoted_path}; eval "${{EDITOR:-vi}} \"\$scrollback_file\""; status=$?; rm -f "$scrollback_file"; exit $status"#
    );
    Ok(vec!["/bin/sh".to_string(), "-c".to_string(), command])
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(
                    ch,
                    '@' | '%' | '_' | '+' | '=' | ':' | ',' | '.' | '/' | '-'
                )
        })
    {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcGroupMember {
    pid: u32,
    comm: String,
}

type ForegroundMembersByGroup = HashMap<u32, Vec<ProcGroupMember>>;

#[derive(Debug, Clone)]
struct CachedForegroundMembers {
    built_at: Instant,
    by_group: ForegroundMembersByGroup,
}

#[derive(Debug, Default)]
struct ForegroundMembersCache {
    cached: Option<CachedForegroundMembers>,
}

static FOREGROUND_MEMBERS_CACHE: Mutex<ForegroundMembersCache> =
    Mutex::new(ForegroundMembersCache { cached: None });

pub fn raise_server_nofile_limit() {}

fn custom_command_argv(command: &str, flag: &str) -> Vec<std::ffi::OsString> {
    vec!["/bin/sh".into(), flag.into(), command.into()]
}

pub(crate) fn detached_custom_command_process_platform(command: &str) -> std::process::Command {
    let argv = custom_command_argv(command, "-lc");
    let mut process = std::process::Command::new(&argv[0]);
    process.args(&argv[1..]);
    process
}

pub(crate) fn pane_custom_command_pty_builder_platform(
    command: &str,
) -> portable_pty::CommandBuilder {
    portable_pty::CommandBuilder::from_argv(custom_command_argv(command, "-c"))
}

/// Collect the foreground terminal job for a given child PID.
pub fn foreground_job(child_pid: u32) -> Option<ForegroundJob> {
    let foreground_pgid = foreground_process_group_id(child_pid);
    let process_group_id = foreground_pgid.unwrap_or(child_pid);
    let mut processes = foreground_pgid
        .and_then(foreground_process_group_members)
        .unwrap_or_default()
        .into_iter()
        .filter_map(foreground_process_from_member)
        .collect::<Vec<_>>();
    let mut seen_pids: std::collections::HashSet<u32> =
        processes.iter().map(|process| process.pid).collect();

    // Some CI/container PTYs do not expose a useful foreground process group
    // while a command is being launched from the pane shell. Only fall back to
    // the pane shell descendants when the foreground group is missing, empty,
    // or just the pane shell itself. A real foreground job should stay
    // authoritative so background agents in the same terminal session cannot
    // steal detection.
    if foreground_pgid.is_none()
        || foreground_group_needs_descendant_fallback(child_pid, &processes)
    {
        for pid in session_processes(child_pid) {
            if seen_pids.contains(&pid) {
                continue;
            }
            if let Some(process) = foreground_process_info(pid) {
                seen_pids.insert(pid);
                processes.push(process);
            }
        }
    }

    if processes.is_empty() {
        return None;
    }

    Some(ForegroundJob {
        process_group_id,
        processes,
    })
}

fn foreground_process_group_members(process_group_id: u32) -> Option<Vec<ProcGroupMember>> {
    let mut cache = FOREGROUND_MEMBERS_CACHE
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    cache.members(
        process_group_id,
        Instant::now(),
        FOREGROUND_MEMBERS_CACHE_TTL,
        build_foreground_members_by_group,
        live_process_group_member,
    )
}

fn live_process_group_member(process_group_id: u32, pid: u32) -> Option<ProcGroupMember> {
    let stat = process_stat(pid)?;
    (stat.pgrp > 0 && stat.pgrp as u32 == process_group_id).then_some(ProcGroupMember {
        pid,
        comm: stat.comm,
    })
}

fn foreground_process_from_member(member: ProcGroupMember) -> Option<ForegroundProcess> {
    let argv = process_argv(member.pid);
    Some(ForegroundProcess {
        pid: member.pid,
        name: member.comm,
        argv0: argv.as_ref().and_then(|parts| parts.first().cloned()),
        cmdline: argv.as_ref().map(|parts| parts.join(" ")),
        argv,
    })
}

fn foreground_group_needs_descendant_fallback(
    child_pid: u32,
    processes: &[ForegroundProcess],
) -> bool {
    processes.is_empty() || processes.iter().all(|process| process.pid == child_pid)
}

pub fn foreground_process_group_id(child_pid: u32) -> Option<u32> {
    // /proc/<pid>/stat format: "pid (comm) state ppid pgrp session tty_nr tpgid ..."
    // The (comm) field can contain spaces and parens, so we find the last ')' first.
    let stat = std::fs::read_to_string(format!("/proc/{child_pid}/stat")).ok()?;
    let rest = stat.get(stat.rfind(')')? + 2..)?;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // After (comm): state(0) ppid(1) pgrp(2) session(3) tty_nr(4) tpgid(5)
    let tpgid: i32 = fields.get(5)?.parse().ok()?;
    (tpgid > 0).then_some(tpgid as u32)
}

pub fn foreground_process_group_id_for_tty_fd(fd: RawFd) -> Option<u32> {
    let pgid = unsafe { libc::tcgetpgrp(fd) };
    (pgid > 0).then_some(pgid as u32)
}

fn foreground_process_info(pid: u32) -> Option<ForegroundProcess> {
    let name = process_stat(pid)?.comm;
    let argv = process_argv(pid);
    Some(ForegroundProcess {
        pid,
        name,
        argv0: argv.as_ref().and_then(|parts| parts.first().cloned()),
        cmdline: argv.as_ref().map(|parts| parts.join(" ")),
        argv,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcStatEntry {
    pid: u32,
    pgrp: i32,
    comm: String,
}

fn build_foreground_members_by_group() -> ForegroundMembersByGroup {
    let entries = std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let file_name = entry.file_name();
            let pid_str = file_name.to_str()?;
            if !pid_str.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            let pid = pid_str.parse::<u32>().ok()?;
            let stat = process_stat(pid)?;
            Some(ProcStatEntry {
                pid,
                pgrp: stat.pgrp,
                comm: stat.comm,
            })
        });
    foreground_members_by_group_from_entries(entries)
}

fn foreground_members_by_group_from_entries(
    entries: impl IntoIterator<Item = ProcStatEntry>,
) -> ForegroundMembersByGroup {
    let mut by_group = ForegroundMembersByGroup::default();
    for entry in entries {
        if entry.pgrp <= 0 {
            continue;
        }
        by_group
            .entry(entry.pgrp as u32)
            .or_default()
            .push(ProcGroupMember {
                pid: entry.pid,
                comm: entry.comm,
            });
    }
    by_group
}

impl ForegroundMembersCache {
    fn members(
        &mut self,
        process_group_id: u32,
        now: Instant,
        max_age: Duration,
        build: impl FnOnce() -> ForegroundMembersByGroup,
        mut validate: impl FnMut(u32, u32) -> Option<ProcGroupMember>,
    ) -> Option<Vec<ProcGroupMember>> {
        if let Some(cached) = &self.cached {
            if now.saturating_duration_since(cached.built_at) < max_age {
                if let Some(members) = cached.by_group.get(&process_group_id) {
                    let members = members
                        .iter()
                        .filter_map(|member| validate(process_group_id, member.pid))
                        .collect::<Vec<_>>();
                    if !members.is_empty() {
                        return Some(members);
                    }
                }
                return self.refresh_and_get(process_group_id, now, build);
            }
        }
        self.refresh_and_get(process_group_id, now, build)
    }

    fn refresh_and_get(
        &mut self,
        process_group_id: u32,
        now: Instant,
        build: impl FnOnce() -> ForegroundMembersByGroup,
    ) -> Option<Vec<ProcGroupMember>> {
        self.cached = Some(CachedForegroundMembers {
            built_at: now,
            by_group: build(),
        });
        self.cached
            .as_ref()?
            .by_group
            .get(&process_group_id)
            .cloned()
    }
}

#[derive(Clone)]
struct ProcStat {
    ppid: i32,
    pgrp: i32,
    session: i32,
    comm: String,
}

fn process_stat(pid: u32) -> Option<ProcStat> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let close = stat.rfind(')')?;
    let comm = stat.get(1 + stat.find('(')?..close)?.to_string();
    let rest = stat.get(close + 2..)?;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let ppid: i32 = fields.get(1)?.parse().ok()?;
    let pgrp: i32 = fields.get(2)?.parse().ok()?;
    let session: i32 = fields.get(3)?.parse().ok()?;
    Some(ProcStat {
        ppid,
        pgrp,
        session,
        comm,
    })
}

fn process_argv(pid: u32) -> Option<Vec<String>> {
    let bytes = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let parts: Vec<String> = bytes
        .split(|&b| b == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect();
    (!parts.is_empty()).then_some(parts)
}

/// Get the current working directory of a process.
/// Uses /proc/<pid>/cwd symlink.
pub fn process_cwd(pid: u32) -> Option<PathBuf> {
    if pid == 0 {
        return None;
    }
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

/// Read Gardn's agent identity hint from a process environment.
pub fn process_agent_hint(pid: u32) -> Option<crate::detect::Agent> {
    if pid == 0 {
        return None;
    }
    let environ = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    super::parse_agent_env_hint(&environ)
}

pub fn active_tcp_listeners() -> Vec<super::TcpListenerInfo> {
    super::active_tcp_listeners_from_lsof()
}

pub fn session_processes(child_pid: u32) -> Vec<u32> {
    let Some(shell_session) = process_stat(child_pid).map(|stat| stat.session) else {
        return Vec::new();
    };

    let mut table = std::collections::HashMap::new();
    for pid in all_pids() {
        let Some(stat) = process_stat(pid) else {
            continue;
        };
        if stat.session == shell_session {
            table.insert(pid, stat);
        }
    }

    let mut pids = table
        .keys()
        .copied()
        .filter(|pid| process_is_descendant_of_child(*pid, child_pid, &table))
        .collect::<Vec<_>>();
    pids.sort_unstable();
    pids.dedup();
    pids
}

fn process_is_descendant_of_child(
    mut pid: u32,
    child_pid: u32,
    table: &std::collections::HashMap<u32, ProcStat>,
) -> bool {
    for _ in 0..=table.len() {
        if pid == child_pid {
            return true;
        }
        let Some(stat) = table.get(&pid) else {
            return false;
        };
        if stat.ppid <= 0 {
            return false;
        }
        pid = stat.ppid as u32;
    }
    false
}

pub fn signal_processes(pids: &[u32], signal: Signal) {
    let sig = match signal {
        Signal::Hangup => libc::SIGHUP,
        Signal::Terminate => libc::SIGTERM,
        Signal::Kill => libc::SIGKILL,
    };

    for &pid in pids {
        if pid == 0 {
            continue;
        }
        unsafe {
            libc::kill(pid as i32, sig);
        }
    }
}

pub fn process_exists(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let result = unsafe { libc::kill(pid as i32, 0) };
    if result == 0 {
        true
    } else {
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

pub fn write_clipboard(bytes: &[u8]) -> bool {
    for command in clipboard_commands() {
        if run_clipboard_command(&command, bytes) {
            return true;
        }
    }
    false
}

pub fn read_clipboard_text() -> Option<String> {
    for command in read_clipboard_text_commands() {
        if let Some(text) = read_clipboard_text_with_command(&command) {
            return Some(text);
        }
    }
    None
}

pub fn open_url(url: &str) -> std::io::Result<()> {
    Command::new("xdg-open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

pub fn read_clipboard_image() -> Option<ClipboardImage> {
    for (mime, extension) in [
        ("image/png", "png"),
        ("image/jpeg", "jpg"),
        ("image/jpg", "jpg"),
        ("image/gif", "gif"),
        ("image/webp", "webp"),
        ("image/bmp", "bmp"),
    ] {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            if let Some(image) =
                read_validated_clipboard_image("wl-paste", &["--type", mime], extension)
            {
                return Some(image);
            }
        }

        if std::env::var_os("DISPLAY").is_some() {
            if let Some(image) = read_validated_clipboard_image(
                "xclip",
                &["-selection", "clipboard", "-t", mime, "-o"],
                extension,
            ) {
                return Some(image);
            }
        }
    }

    None
}

fn read_validated_clipboard_image(
    program: &str,
    args: &[&str],
    extension: &'static str,
) -> Option<ClipboardImage> {
    let bytes = read_clipboard_image_with_command(program, args)?;
    if !bytes_match_image_signature(extension, &bytes) {
        return None;
    }
    Some(ClipboardImage { bytes, extension })
}

fn bytes_match_image_signature(extension: &str, bytes: &[u8]) -> bool {
    match extension {
        "png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "jpg" => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && bytes[8..12] == *b"WEBP",
        "bmp" => {
            if bytes.len() < 26 || !bytes.starts_with(b"BM") {
                return false;
            }
            let offset = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]) as usize;
            (26..=bytes.len()).contains(&offset)
        }
        _ => false,
    }
}

/// Show a native desktop notification through libnotify's command-line helper.
pub fn show_desktop_notification(title: &str, body: Option<&str>) -> std::io::Result<bool> {
    show_desktop_notification_with_command(title, body, |program| Command::new(program))
}

pub fn menu_extra_is_running() -> bool {
    false
}

fn show_desktop_notification_with_command(
    title: &str,
    body: Option<&str>,
    mut command: impl FnMut(&str) -> Command,
) -> std::io::Result<bool> {
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Ok(false);
    }

    let mut cmd = command("notify-send");
    cmd.arg("--").arg(title);
    if let Some(body) = body.filter(|body| !body.is_empty()) {
        cmd.arg(body);
    }
    run_notification_command(cmd)
}

fn run_notification_command(mut command: Command) -> std::io::Result<bool> {
    let status = match command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) => status,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };

    Ok(status.success())
}

fn read_clipboard_image_with_command(program: &str, args: &[&str]) -> Option<Vec<u8>> {
    let mut command = Command::new(program);
    command.args(args);
    read_clipboard_image_with_spawned_command(command)
}

fn read_clipboard_image_with_spawned_command(command: Command) -> Option<Vec<u8>> {
    read_clipboard_image_with_spawned_command_max(
        command,
        crate::protocol::MAX_CLIPBOARD_IMAGE_PAYLOAD,
    )
}

fn read_clipboard_image_with_spawned_command_max(
    mut command: Command,
    max_bytes: usize,
) -> Option<Vec<u8>> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;

    let read = match read_limited_reader(stdout, max_bytes) {
        Ok(read) => read,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    };

    if read == LimitedRead::Oversized {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }

    let status = child.wait().ok()?;
    if !status.success() {
        return None;
    }

    match read {
        LimitedRead::Complete(bytes) => Some(bytes),
        LimitedRead::Empty | LimitedRead::Oversized => None,
    }
}

fn clipboard_commands() -> Vec<ClipboardCommand> {
    let mut commands = Vec::new();

    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        commands.push(ClipboardCommand {
            program: "wl-copy",
            args: &["--type", "text/plain;charset=utf-8"],
        });
    }

    if std::env::var_os("DISPLAY").is_some() {
        commands.push(ClipboardCommand {
            program: "xclip",
            args: &["-selection", "clipboard", "-in"],
        });
        commands.push(ClipboardCommand {
            program: "xsel",
            args: &["--clipboard", "--input"],
        });
    }

    commands
}

fn read_clipboard_text_commands() -> Vec<ClipboardCommand> {
    let mut commands = Vec::new();

    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        commands.push(ClipboardCommand {
            program: "wl-paste",
            args: &["--type", "text/plain;charset=utf-8"],
        });
        commands.push(ClipboardCommand {
            program: "wl-paste",
            args: &["--type", "text/plain"],
        });
    }

    if std::env::var_os("DISPLAY").is_some() {
        commands.push(ClipboardCommand {
            program: "xclip",
            args: &["-selection", "clipboard", "-out"],
        });
        commands.push(ClipboardCommand {
            program: "xsel",
            args: &["--clipboard", "--output"],
        });
    }

    commands
}

fn read_clipboard_text_with_command(command: &ClipboardCommand) -> Option<String> {
    const MAX_CLIPBOARD_TEXT_BYTES: usize = 1024 * 1024;

    let mut child = Command::new(command.program)
        .args(command.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let stdout = child.stdout.take()?;
    let read = match read_limited_reader(stdout, MAX_CLIPBOARD_TEXT_BYTES) {
        Ok(LimitedRead::Oversized) => {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        Ok(read) => read,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    };

    let status = child.wait().ok()?;
    if !status.success() {
        return None;
    }

    match read {
        LimitedRead::Complete(bytes) => String::from_utf8(bytes).ok(),
        LimitedRead::Empty => None,
        LimitedRead::Oversized => unreachable!("oversized clipboard text is handled before wait"),
    }
}

fn run_clipboard_command(command: &ClipboardCommand, bytes: &[u8]) -> bool {
    let mut child = match Command::new(command.program)
        .args(command.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };

    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return false;
    };

    if stdin.write_all(bytes).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return false;
    }
    drop(stdin);

    child.wait().map(|status| status.success()).unwrap_or(false)
}

fn all_pids() -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .collect()
}

/// The machine's node name, as shown by tmux's `#h`.
pub(crate) fn hostname_platform() -> Option<String> {
    let mut buffer = [0_u8; 256];
    let result =
        unsafe { libc::gethostname(buffer.as_mut_ptr().cast::<libc::c_char>(), buffer.len()) };
    if result != 0 {
        return None;
    }
    let end = buffer
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(buffer.len());
    let name = String::from_utf8_lossy(&buffer[..end]).into_owned();
    (!name.is_empty()).then_some(name)
}
#[cfg(test)]
mod tests {
    use super::super::parse_agent_env_hint;
    use super::*;
    use crate::config::TestEnvVar;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, OnceLock};

    use std::os::unix::process::CommandExt;
    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn parse_agent_env_hint_accepts_gardn_agent_values() {
        assert_eq!(
            parse_agent_env_hint(b"PATH=/bin\0GARDN_AGENT=claude\0TERM=xterm\0"),
            Some(crate::detect::Agent::Claude)
        );
        assert_eq!(
            parse_agent_env_hint(b"GARDN_AGENT=codex"),
            Some(crate::detect::Agent::Codex)
        );
        assert_eq!(
            parse_agent_env_hint(b"GARDN_AGENT=pi\0"),
            Some(crate::detect::Agent::Pi)
        );
    }

    #[test]
    fn parse_agent_env_hint_ignores_unknown_or_invalid_values() {
        assert_eq!(parse_agent_env_hint(b"PATH=/bin\0TERM=xterm\0"), None);
        assert_eq!(parse_agent_env_hint(b"GARDN_AGENT=not-an-agent\0"), None);
        assert_eq!(parse_agent_env_hint(b"GARDN_AGENT=\xff\0"), None);
    }

    #[test]
    fn wsl_marker_detection_matches_kernel_release_text() {
        assert!(text_indicates_wsl("5.15.167.4-microsoft-standard-WSL2"));
        assert!(text_indicates_wsl("4.4.0-19041-Microsoft"));
        assert!(!text_indicates_wsl("6.8.0-64-generic"));
        assert!(!text_indicates_wsl(""));
    }

    fn proc_entry(pid: u32, pgrp: i32, comm: &str) -> ProcStatEntry {
        ProcStatEntry {
            pid,
            pgrp,
            comm: comm.to_string(),
        }
    }

    fn foreground_members(groups: &[(u32, &str, i32)]) -> ForegroundMembersByGroup {
        foreground_members_by_group_from_entries(
            groups
                .iter()
                .map(|(pid, comm, pgrp)| proc_entry(*pid, *pgrp, comm)),
        )
    }

    fn validate_from<'a>(
        groups: &'a [(u32, &'a str, i32)],
    ) -> impl FnMut(u32, u32) -> Option<ProcGroupMember> + 'a {
        move |process_group_id, pid| {
            groups.iter().find_map(|(member_pid, comm, pgrp)| {
                (*member_pid == pid && *pgrp > 0 && *pgrp as u32 == process_group_id).then(|| {
                    ProcGroupMember {
                        pid,
                        comm: (*comm).to_string(),
                    }
                })
            })
        }
    }

    #[test]
    fn foreground_members_index_processes_by_group() {
        let by_group = foreground_members(&[
            (10, "shell", 10),
            (11, "agent", 11),
            (12, "worker", 11),
            (13, "ignored", -1),
        ]);

        assert_eq!(
            by_group
                .get(&11)
                .unwrap()
                .iter()
                .map(|member| member.comm.as_str())
                .collect::<Vec<_>>(),
            vec!["agent", "worker"]
        );
        assert!(!by_group.contains_key(&13));
    }

    #[test]
    fn foreground_members_cache_reuses_live_snapshot_inside_ttl() {
        let mut cache = ForegroundMembersCache::default();
        let now = Instant::now();
        let builds = AtomicUsize::new(0);

        let first = cache.members(
            10,
            now,
            FOREGROUND_MEMBERS_CACHE_TTL,
            || {
                builds.fetch_add(1, Ordering::Relaxed);
                foreground_members(&[(10, "shell", 10)])
            },
            validate_from(&[(10, "shell", 10)]),
        );
        let second = cache.members(
            10,
            now + Duration::from_millis(100),
            FOREGROUND_MEMBERS_CACHE_TTL,
            || {
                builds.fetch_add(1, Ordering::Relaxed);
                foreground_members(&[(20, "new", 20)])
            },
            validate_from(&[(10, "shell-live", 10)]),
        );

        assert_eq!(first.unwrap()[0].comm, "shell");
        assert_eq!(second.unwrap()[0].comm, "shell-live");
        assert_eq!(builds.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn foreground_members_cache_rebuilds_after_ttl() {
        let mut cache = ForegroundMembersCache::default();
        let now = Instant::now();
        let builds = AtomicUsize::new(0);

        let _ = cache.members(
            10,
            now,
            FOREGROUND_MEMBERS_CACHE_TTL,
            || {
                builds.fetch_add(1, Ordering::Relaxed);
                foreground_members(&[(10, "old", 10)])
            },
            validate_from(&[(10, "old", 10)]),
        );
        let refreshed = cache.members(
            20,
            now + FOREGROUND_MEMBERS_CACHE_TTL,
            FOREGROUND_MEMBERS_CACHE_TTL,
            || {
                builds.fetch_add(1, Ordering::Relaxed);
                foreground_members(&[(20, "new", 20)])
            },
            validate_from(&[(20, "new", 20)]),
        );

        assert_eq!(refreshed.unwrap()[0].comm, "new");
        assert_eq!(builds.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn foreground_members_cache_refreshes_when_cached_group_members_exit() {
        let mut cache = ForegroundMembersCache::default();
        let now = Instant::now();
        let builds = AtomicUsize::new(0);

        let _ = cache.members(
            42,
            now,
            FOREGROUND_MEMBERS_CACHE_TTL,
            || {
                builds.fetch_add(1, Ordering::Relaxed);
                foreground_members(&[(10, "old", 42)])
            },
            validate_from(&[(10, "old", 42)]),
        );
        let refreshed = cache.members(
            42,
            now + Duration::from_millis(10),
            FOREGROUND_MEMBERS_CACHE_TTL,
            || {
                builds.fetch_add(1, Ordering::Relaxed);
                foreground_members(&[(20, "new", 42)])
            },
            validate_from(&[(10, "old", 7), (20, "new", 42)]),
        );

        assert_eq!(refreshed.unwrap()[0].comm, "new");
        assert_eq!(builds.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn session_processes_are_scoped_to_pane_session() {
        let mut command = std::process::Command::new("/bin/sh");
        command.arg("-c").arg("sleep 5");
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn().expect("spawn shell");

        let pids = session_processes(child.id());
        let _ = child.kill();
        let _ = child.wait();

        assert!(pids.contains(&child.id()));
        assert!(!pids.contains(&std::process::id()));
    }

    #[test]
    fn process_descendant_check_ignores_same_session_siblings() {
        let table = std::collections::HashMap::from([
            (
                10,
                ProcStat {
                    ppid: 1,
                    pgrp: 10,
                    session: 10,
                    comm: "sh".into(),
                },
            ),
            (
                11,
                ProcStat {
                    ppid: 10,
                    pgrp: 11,
                    session: 10,
                    comm: "pi".into(),
                },
            ),
            (
                12,
                ProcStat {
                    ppid: 1,
                    pgrp: 12,
                    session: 10,
                    comm: "codex".into(),
                },
            ),
        ]);

        assert!(process_is_descendant_of_child(11, 10, &table));
        assert!(!process_is_descendant_of_child(12, 10, &table));
    }

    #[test]
    fn clipboard_commands_prefer_wayland_when_available() {
        let _guard = env_lock().lock().unwrap();
        let _wayland_env = TestEnvVar::set("WAYLAND_DISPLAY", "wayland-0");
        let _display_env = TestEnvVar::remove("DISPLAY");
        let commands = clipboard_commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].program, "wl-copy");
    }

    #[test]
    fn clipboard_commands_include_x11_fallbacks() {
        let _guard = env_lock().lock().unwrap();
        let _wayland_env = TestEnvVar::remove("WAYLAND_DISPLAY");
        let _display_env = TestEnvVar::set("DISPLAY", ":0");
        let commands = clipboard_commands();
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].program, "xclip");
        assert_eq!(commands[1].program, "xsel");
    }

    #[test]
    fn read_clipboard_text_commands_include_session_backends() {
        let _guard = env_lock().lock().unwrap();
        let _wayland_env = TestEnvVar::set("WAYLAND_DISPLAY", "wayland-0");
        let _display_env = TestEnvVar::set("DISPLAY", ":0");

        let commands = read_clipboard_text_commands();
        assert_eq!(commands[0].program, "wl-paste");
        assert_eq!(commands[1].program, "wl-paste");
        assert_eq!(commands[2].program, "xclip");
        assert_eq!(commands[3].program, "xsel");
    }

    #[test]
    fn read_clipboard_text_with_command_reads_utf8() {
        let command = ClipboardCommand {
            program: "printf",
            args: &["feature/linear-302"],
        };

        assert_eq!(
            read_clipboard_text_with_command(&command).as_deref(),
            Some("feature/linear-302")
        );
    }

    #[test]
    fn read_clipboard_text_with_command_rejects_oversized_output() {
        let command = ClipboardCommand {
            program: "sh",
            args: &["-c", "yes x | head -c 1048578"],
        };

        assert_eq!(read_clipboard_text_with_command(&command), None);
    }

    #[test]
    fn read_clipboard_image_with_spawned_command_reads_under_limit() {
        let mut command = Command::new("sh");
        command.arg("-c").arg("printf image");

        assert_eq!(
            read_clipboard_image_with_spawned_command_max(command, 16),
            Some(b"image".to_vec())
        );
    }

    #[test]
    fn read_clipboard_image_with_spawned_command_rejects_over_limit() {
        let mut command = Command::new("sh");
        command.arg("-c").arg("printf oversized");

        assert_eq!(
            read_clipboard_image_with_spawned_command_max(command, 4),
            None
        );
    }

    #[test]
    fn read_clipboard_image_rejects_xclip_text_served_for_image_target() {
        let _guard = env_lock().lock().unwrap();
        let temp_dir =
            std::env::temp_dir().join(format!("gardn-fake-xclip-{}", std::process::id()));
        std::fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let fake_xclip = temp_dir.join("xclip");
        std::fs::write(&fake_xclip, "#!/bin/sh\nprintf '# Tasks'\n")
            .expect("fake xclip should be written");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = std::fs::metadata(&fake_xclip)
                .expect("fake xclip metadata")
                .permissions();
            permissions.set_mode(0o700);
            std::fs::set_permissions(&fake_xclip, permissions)
                .expect("fake xclip should be executable");
        }

        let test_path = {
            let mut paths = vec![temp_dir.clone()];
            if let Some(path) = std::env::var_os("PATH") {
                paths.extend(std::env::split_paths(&path));
            }
            std::env::join_paths(paths).expect("test path should be valid")
        };

        let _wayland_env = TestEnvVar::remove("WAYLAND_DISPLAY");
        let _display_env = TestEnvVar::set("DISPLAY", ":0");
        let _path_env = TestEnvVar::set("PATH", test_path);

        let result = read_clipboard_image();

        let _ = std::fs::remove_file(fake_xclip);
        let _ = std::fs::remove_dir(temp_dir);

        assert_eq!(result, None);
    }

    #[test]
    fn read_clipboard_image_rejects_wayland_xclip_fallback_text_for_image_target() {
        let _guard = env_lock().lock().unwrap();
        let temp_dir =
            std::env::temp_dir().join(format!("gardn-fake-wayland-xclip-{}", std::process::id()));
        std::fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let fake_wl_paste = temp_dir.join("wl-paste");
        let fake_xclip = temp_dir.join("xclip");
        std::fs::write(&fake_wl_paste, "#!/bin/sh\nexit 1\n")
            .expect("fake wl-paste should be written");
        std::fs::write(&fake_xclip, "#!/bin/sh\nprintf '# Tasks'\n")
            .expect("fake xclip should be written");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            for command in [&fake_wl_paste, &fake_xclip] {
                let mut permissions = std::fs::metadata(command)
                    .expect("fake clipboard command metadata")
                    .permissions();
                permissions.set_mode(0o700);
                std::fs::set_permissions(command, permissions)
                    .expect("fake clipboard command should be executable");
            }
        }

        let test_path = {
            let mut paths = vec![temp_dir.clone()];
            if let Some(path) = std::env::var_os("PATH") {
                paths.extend(std::env::split_paths(&path));
            }
            std::env::join_paths(paths).expect("test path should be valid")
        };

        let _wayland_env = TestEnvVar::set("WAYLAND_DISPLAY", "wayland-0");
        let _display_env = TestEnvVar::set("DISPLAY", ":0");
        let _path_env = TestEnvVar::set("PATH", test_path);

        let result = read_clipboard_image();

        let _ = std::fs::remove_file(fake_wl_paste);
        let _ = std::fs::remove_file(fake_xclip);
        let _ = std::fs::remove_dir(temp_dir);

        assert_eq!(result, None);
    }

    #[test]
    fn read_validated_clipboard_image_accepts_real_png_payload() {
        assert_eq!(
            read_validated_clipboard_image(
                "sh",
                &["-c", "printf '\\211PNG\\r\\n\\032\\nrest-of-image'"],
                "png"
            ),
            Some(ClipboardImage {
                bytes: b"\x89PNG\r\n\x1a\nrest-of-image".to_vec(),
                extension: "png",
            })
        );
    }

    #[test]
    fn image_signatures_match_only_their_format() {
        assert!(bytes_match_image_signature("png", b"\x89PNG\r\n\x1a\n..."));
        assert!(bytes_match_image_signature(
            "jpg",
            &[0xFF, 0xD8, 0xFF, 0xE0]
        ));
        assert!(bytes_match_image_signature("gif", b"GIF87a..."));
        assert!(bytes_match_image_signature("gif", b"GIF89a..."));
        assert!(bytes_match_image_signature(
            "webp",
            b"RIFF\x10\x00\x00\x00WEBPVP8 "
        ));

        let mut bmp = vec![0u8; 26];
        bmp[..2].copy_from_slice(b"BM");
        bmp[10] = 26;
        assert!(bytes_match_image_signature("bmp", &bmp));

        assert!(!bytes_match_image_signature("png", b"# Tasks"));
        assert!(!bytes_match_image_signature("jpg", b"plain clipboard text"));
        assert!(!bytes_match_image_signature("gif", b""));
        assert!(!bytes_match_image_signature("webp", b"RIFF but not webp"));
        assert!(!bytes_match_image_signature("bmp", b"\x89PNG\r\n\x1a\n"));
        assert!(!bytes_match_image_signature(
            "bmp",
            b"BM text is not a bitmap"
        ));
        assert!(!bytes_match_image_signature("svg", b"<svg></svg>"));
    }

    #[test]
    fn desktop_notification_separates_option_like_titles() {
        let _guard = env_lock().lock().unwrap();
        let _wayland_env = TestEnvVar::remove("WAYLAND_DISPLAY");
        let _display_env = TestEnvVar::set("DISPLAY", ":0");

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "gardn-notify-send-args-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let script = "printf '%s\\n' \"$@\" > \"$GARDN_NOTIFY_ARGS\"";
        let shown = show_desktop_notification_with_command("-danger", Some("body"), |_| {
            let mut cmd = Command::new("sh");
            cmd.arg("-c")
                .arg(script)
                .arg("notify-send")
                .env("GARDN_NOTIFY_ARGS", &path);
            cmd
        })
        .expect("notification command should run");

        assert!(shown);
        let args = std::fs::read_to_string(&path).expect("args file");
        let _ = std::fs::remove_file(&path);
        assert_eq!(args, "--\n-danger\nbody\n");
    }
}
