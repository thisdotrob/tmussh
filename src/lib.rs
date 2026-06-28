use std::process::{Command, ExitStatus, Stdio};

use color_eyre::eyre::{Result, WrapErr, bail, eyre};

const TMUX_CANDIDATES: &[&str] = &[
    "/opt/homebrew/bin/tmux",
    "/usr/local/bin/tmux",
    "/usr/bin/tmux",
    "/bin/tmux",
    "/snap/bin/tmux",
    "/run/current-system/sw/bin/tmux",
    "$HOME/.nix-profile/bin/tmux",
    "$HOME/.local/bin/tmux",
    "$HOME/bin/tmux",
];

pub const DEFAULT_REMOTE_TERM: &str = "xterm-256color";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxAction {
    Attach(String),
    New(String),
    Quit,
}

pub fn parse_session_list(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub fn validate_new_session_name(name: &str) -> std::result::Result<(), &'static str> {
    if name.is_empty() {
        return Err("Session name cannot be empty");
    }

    if name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-'))
    {
        Ok(())
    } else {
        Err("Use only letters, numbers, underscores, periods, and hyphens")
    }
}

pub fn list_sessions(
    destination: &str,
    tmux_path: Option<&str>,
    remote_term: &str,
) -> Result<Vec<String>> {
    let output = Command::new("ssh")
        .arg(destination)
        .arg(list_sessions_command(tmux_path, remote_term))
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .wrap_err("failed to run ssh")?;

    classify_list_sessions_output(
        output.status.success(),
        &String::from_utf8_lossy(&output.stdout),
        &String::from_utf8_lossy(&output.stderr),
    )
}

pub fn classify_list_sessions_output(
    success: bool,
    stdout: &str,
    stderr: &str,
) -> Result<Vec<String>> {
    let combined = format!("{stdout}\n{stderr}");
    if is_no_tmux_server_error(&combined) {
        return Ok(Vec::new());
    }

    if success {
        return Ok(parse_session_list(stdout));
    }

    let detail = combined.trim();
    if detail.is_empty() {
        bail!("failed to list tmux sessions on the remote host");
    }

    bail!("failed to list tmux sessions on the remote host: {detail}");
}

pub fn run_remote_tmux(
    destination: &str,
    action: &TmuxAction,
    tmux_path: Option<&str>,
    remote_term: &str,
) -> Result<ExitStatus> {
    let command = match action {
        TmuxAction::Attach(session) => attach_command(session, tmux_path, remote_term),
        TmuxAction::New(session) => new_session_command(session, tmux_path, remote_term),
        TmuxAction::Quit => return Err(eyre!("cannot run a quit action")),
    };

    Command::new("ssh")
        .arg("-t")
        .arg(destination)
        .arg(command)
        .status()
        .wrap_err("failed to run ssh")
}

pub fn attach_command(session: &str, tmux_path: Option<&str>, remote_term: &str) -> String {
    remote_tmux_command(
        tmux_path,
        &format!("attach-session -t {}", shell_quote(session)),
        remote_term,
    )
}

pub fn new_session_command(session: &str, tmux_path: Option<&str>, remote_term: &str) -> String {
    remote_tmux_command(
        tmux_path,
        &format!("new-session -A -s {}", shell_quote(session)),
        remote_term,
    )
}

pub fn list_sessions_command(tmux_path: Option<&str>, remote_term: &str) -> String {
    remote_tmux_command(tmux_path, "list-sessions -F '#S' 2>&1", remote_term)
}

pub fn remote_tmux_command(tmux_path: Option<&str>, tmux_args: &str, remote_term: &str) -> String {
    let script = format!(
        "{}TERM={}; export TERM; exec \"$tmussh_tmux\" -u {tmux_args}",
        tmux_lookup_script(tmux_path),
        shell_quote(remote_term),
    );

    format!("/bin/sh -lc {}", shell_quote(&script))
}

pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'\''"#))
}

fn tmux_lookup_script(tmux_path: Option<&str>) -> String {
    if let Some(tmux_path) = tmux_path {
        return format!(
            concat!(
                "tmussh_tmux={}; ",
                "if [ ! -x \"$tmussh_tmux\" ]; then ",
                "printf '%s\\n' \"tmussh: tmux is not executable at $tmussh_tmux\"; ",
                "exit 127; ",
                "fi; "
            ),
            shell_quote(tmux_path)
        );
    }

    format!(
        concat!(
            "tmussh_tmux=$(command -v tmux 2>/dev/null || true); ",
            "if [ -z \"$tmussh_tmux\" ]; then ",
            "for tmussh_candidate in {}; do ",
            "if [ -x \"$tmussh_candidate\" ]; then ",
            "tmussh_tmux=$tmussh_candidate; ",
            "break; ",
            "fi; ",
            "done; ",
            "fi; ",
            "if [ -z \"$tmussh_tmux\" ]; then ",
            "printf '%s\\n' ",
            "\"tmussh: tmux not found on remote host; install tmux or run tmussh --tmux-path /path/to/tmux\"; ",
            "exit 127; ",
            "fi; "
        ),
        tmux_candidate_list()
    )
}

fn tmux_candidate_list() -> String {
    TMUX_CANDIDATES
        .iter()
        .map(|candidate| shell_quote(candidate))
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_no_tmux_server_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("no server running")
        || message.contains("failed to connect to server")
        || (message.contains("error connecting to")
            && message.contains("no such file or directory"))
        || message.contains("no sessions")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_session_list() {
        assert_eq!(
            parse_session_list("work\nops\r\n\nmisc.tools\n"),
            vec!["work", "ops", "misc.tools"]
        );
    }

    #[test]
    fn classifies_no_tmux_server_as_empty() {
        let sessions =
            classify_list_sessions_output(false, "", "no server running on /tmp/tmux-1000/default")
                .unwrap();

        assert!(sessions.is_empty());
    }

    #[test]
    fn classifies_missing_tmux_socket_as_empty() {
        let message =
            "error connecting to /private/tmp/tmux-501/default (No such file or directory)";

        let sessions = classify_list_sessions_output(false, message, "").unwrap();
        assert!(sessions.is_empty());

        let sessions = classify_list_sessions_output(true, message, "").unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn surfaces_missing_tmux_as_error() {
        let error = classify_list_sessions_output(false, "", "sh: 1: tmux: not found")
            .unwrap_err()
            .to_string();

        assert!(error.contains("tmux: not found"));
    }

    #[test]
    fn surfaces_ssh_failures_as_errors() {
        let error = classify_list_sessions_output(
            false,
            "",
            "ssh: Could not resolve hostname example.invalid",
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("Could not resolve hostname"));
    }

    #[test]
    fn validates_new_session_names() {
        assert!(validate_new_session_name("work_2026.06-prod").is_ok());
        assert!(validate_new_session_name("").is_err());
        assert!(validate_new_session_name("work session").is_err());
        assert!(validate_new_session_name("prod;rm").is_err());
    }

    #[test]
    fn builds_remote_commands_with_shell_quoting() {
        assert_eq!(
            attach_command("work", Some("/custom/bin/tmux"), DEFAULT_REMOTE_TERM),
            r#"/bin/sh -lc 'tmussh_tmux='\''/custom/bin/tmux'\''; if [ ! -x "$tmussh_tmux" ]; then printf '\''%s\n'\'' "tmussh: tmux is not executable at $tmussh_tmux"; exit 127; fi; TERM='\''xterm-256color'\''; export TERM; exec "$tmussh_tmux" -u attach-session -t '\''work'\'''"#
        );
        assert_eq!(
            attach_command("ops'prod", Some("/custom/bin/tmux"), DEFAULT_REMOTE_TERM),
            r#"/bin/sh -lc 'tmussh_tmux='\''/custom/bin/tmux'\''; if [ ! -x "$tmussh_tmux" ]; then printf '\''%s\n'\'' "tmussh: tmux is not executable at $tmussh_tmux"; exit 127; fi; TERM='\''xterm-256color'\''; export TERM; exec "$tmussh_tmux" -u attach-session -t '\''ops'\''\'\'''\''prod'\'''"#
        );
        assert_eq!(
            new_session_command("new-work", Some("/custom/bin/tmux"), DEFAULT_REMOTE_TERM),
            r#"/bin/sh -lc 'tmussh_tmux='\''/custom/bin/tmux'\''; if [ ! -x "$tmussh_tmux" ]; then printf '\''%s\n'\'' "tmussh: tmux is not executable at $tmussh_tmux"; exit 127; fi; TERM='\''xterm-256color'\''; export TERM; exec "$tmussh_tmux" -u new-session -A -s '\''new-work'\'''"#
        );
        assert_eq!(
            attach_command("work", Some("/custom/bin/tmux"), "xterm-ghostty"),
            r#"/bin/sh -lc 'tmussh_tmux='\''/custom/bin/tmux'\''; if [ ! -x "$tmussh_tmux" ]; then printf '\''%s\n'\'' "tmussh: tmux is not executable at $tmussh_tmux"; exit 127; fi; TERM='\''xterm-ghostty'\''; export TERM; exec "$tmussh_tmux" -u attach-session -t '\''work'\'''"#
        );
    }

    #[test]
    fn list_command_searches_common_tmux_locations() {
        let command = list_sessions_command(None, DEFAULT_REMOTE_TERM);

        assert!(command.starts_with("/bin/sh -lc "));
        assert!(command.contains("command -v tmux"));
        assert!(command.contains("/opt/homebrew/bin/tmux"));
        assert!(command.contains("/usr/local/bin/tmux"));
        assert!(command.contains("/run/current-system/sw/bin/tmux"));
        assert!(command.contains("$HOME/.nix-profile/bin/tmux"));
        assert_eq!(
            list_sessions_command(Some("/custom/bin/tmux"), DEFAULT_REMOTE_TERM),
            r#"/bin/sh -lc 'tmussh_tmux='\''/custom/bin/tmux'\''; if [ ! -x "$tmussh_tmux" ]; then printf '\''%s\n'\'' "tmussh: tmux is not executable at $tmussh_tmux"; exit 127; fi; TERM='\''xterm-256color'\''; export TERM; exec "$tmussh_tmux" -u list-sessions -F '\''#S'\'' 2>&1'"#
        );
    }

    #[test]
    fn generated_remote_command_is_shell_parseable() {
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(remote_tmux_command(
                Some("/bin/echo"),
                &format!("hello {}", shell_quote("ops'prod")),
                DEFAULT_REMOTE_TERM,
            ))
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "-u hello ops'prod\n"
        );
    }
}
