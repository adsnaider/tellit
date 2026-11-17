//! Safe editor resolution and non-shell process launching for `POST /api/open-in-editor`.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;

use tellit_types::{OpenInEditorRequest, OpenInEditorResponse};
use thiserror::Error;
use tokio::process::Command;

use super::diff_parser::COMMIT_MESSAGE_PATH;
use super::jujutsu::validate_repo_relative_path;

/// Built-in editor CLI preset recognized by `tellit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorPreset {
    /// Stable lowercase identifier (`vscode`, `cursor`, `zed`, etc.).
    pub id: &'static str,
    /// Human-readable label shown in settings.
    pub label: &'static str,
    /// Executable name launched directly without a shell.
    pub command: &'static str,
    /// Whitespace-separated argument template supporting `%file` and `%line`.
    pub args_template: &'static str,
}

/// Supported built-in editor presets matching the original desktop/CLI defaults.
pub const EDITOR_PRESETS: &[EditorPreset] = &[
    EditorPreset {
        id: "vscode",
        label: "VS Code",
        command: "code",
        args_template: "-g %file:%line",
    },
    EditorPreset {
        id: "cursor",
        label: "Cursor",
        command: "cursor",
        args_template: "-g %file:%line",
    },
    EditorPreset {
        id: "zed",
        label: "Zed",
        command: "zed",
        args_template: "%file:%line",
    },
    EditorPreset {
        id: "textmate",
        label: "TextMate",
        command: "mate",
        args_template: "-l %line %file",
    },
    EditorPreset {
        id: "bbedit",
        label: "BBEdit",
        command: "bbedit",
        args_template: "+%line %file",
    },
    EditorPreset {
        id: "emacs",
        label: "Emacs",
        command: "emacsclient",
        args_template: "--no-wait +%line %file",
    },
    EditorPreset {
        id: "macvim",
        label: "MacVim",
        command: "mvim",
        args_template: "--remote-silent +%line %file",
    },
    EditorPreset {
        id: "sublime",
        label: "Sublime Text",
        command: "subl",
        args_template: "%file:%line",
    },
    EditorPreset {
        id: "nova",
        label: "Nova",
        command: "nova",
        args_template: "open %file -l %line",
    },
    EditorPreset {
        id: "vim",
        label: "Vim / Neovim",
        command: "nvim",
        args_template: "+%line %file",
    },
];

const BLOCKED_INTERPRETER_NAMES: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "dash",
    "fish",
    "ksh",
    "csh",
    "tcsh",
    "ash",
    "cmd",
    "powershell",
    "pwsh",
    "env",
    "sudo",
    "su",
    "osascript",
    "python",
    "python2",
    "python3",
    "perl",
    "ruby",
    "node",
    "awk",
];

/// Errors returned when validating or launching an external editor.
#[derive(Debug, Error)]
pub enum EditorError {
    /// User configuration set editor preset to `"none"`.
    #[error("open in editor is disabled")]
    Disabled,
    /// Editor command or arguments template is missing or unconfigured.
    #[error("{}", if *.is_custom { "custom editor is not configured; set command and args template in settings" } else { "open in editor is not configured" })]
    NotConfigured {
        /// Whether the `"custom"` preset was selected.
        is_custom: bool,
    },
    /// Supplied file path, command, or template failed validation.
    #[error("{0}")]
    InvalidInput(String),
    /// Resolved target path escapes the repository root (for example via symlink).
    #[error("file path cannot escape the repository root")]
    PathEscapesRepo,
    /// Spawning the validated editor binary failed.
    #[error("failed to launch editor '{command}': {source}")]
    SpawnFailed {
        /// Validated executable name or path.
        command: String,
        /// Underlying OS spawn error.
        #[source]
        source: io::Error,
    },
}

/// Resolved executable and argv list ready for direct `execvp` spawn without a shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorSpawnSpec {
    /// Validated executable name or path.
    pub command: String,
    /// Expanded argv arguments passed to the executable.
    pub args: Vec<String>,
}

/// Looks up a built-in [`EditorPreset`] by case-insensitive identifier.
#[must_use]
pub fn resolve_preset(id: &str) -> Option<&'static EditorPreset> {
    let normalized = id.trim();
    EDITOR_PRESETS
        .iter()
        .find(|preset| preset.id.eq_ignore_ascii_case(normalized))
}

/// Tokenizes an editor arguments template supporting single/double quotes and `\"` / `\\` escapes.
///
/// # Errors
///
/// Returns [`EditorError::InvalidInput`] if a quoted segment is left unterminated or contains NUL.
pub fn parse_editor_args_template(template: &str) -> Result<Vec<String>, EditorError> {
    if template.contains('\0') {
        return Err(EditorError::InvalidInput(String::from(
            "editor arguments template cannot contain NUL bytes",
        )));
    }

    let chars: Vec<char> = template.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0usize;

    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }

        let mut token = String::new();
        let mut quote: Option<char> = None;

        while i < chars.len() {
            let ch = chars[i];
            match quote {
                Some('"') => {
                    if ch == '\\' && i + 1 < chars.len() && matches!(chars[i + 1], '"' | '\\') {
                        token.push(chars[i + 1]);
                        i += 2;
                    } else if ch == '"' {
                        quote = None;
                        i += 1;
                    } else {
                        token.push(ch);
                        i += 1;
                    }
                }
                Some('\'') => {
                    if ch == '\'' {
                        quote = None;
                    } else {
                        token.push(ch);
                    }
                    i += 1;
                }
                Some(_) => unreachable!(),
                None => {
                    if ch == '"' || ch == '\'' {
                        quote = Some(ch);
                        i += 1;
                    } else if ch.is_whitespace() {
                        break;
                    } else {
                        token.push(ch);
                        i += 1;
                    }
                }
            }
        }

        if quote.is_some() {
            return Err(EditorError::InvalidInput(String::from(
                "unterminated quote in editor arguments template",
            )));
        }

        tokens.push(token);
    }

    Ok(tokens)
}

/// Validates and constructs an [`EditorSpawnSpec`] from `req` and canonicalized `file_path`.
///
/// # Errors
///
/// Returns [`EditorError`] if editor launching is disabled, misconfigured, or uses unsafe tokens.
pub fn build_spawn_spec(
    req: &OpenInEditorRequest,
    resolved_file_path: &Path,
) -> Result<EditorSpawnSpec, EditorError> {
    let env_editor = std::env::var("TELLIT_EDITOR")
        .ok()
        .or_else(|| std::env::var("DIFIT_EDITOR").ok())
        .or_else(|| std::env::var("EDITOR").ok());

    let has_explicit_spec = req.editor.as_ref().is_some_and(|e| {
        e.id.as_deref().is_some_and(|s| !s.trim().is_empty())
            || e.command.as_deref().is_some_and(|s| !s.trim().is_empty())
            || e.args_template
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
    });

    let requested_id = req
        .editor
        .as_ref()
        .and_then(|e| e.id.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            (!has_explicit_spec)
                .then_some(env_editor)
                .flatten()
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        });

    if requested_id
        .as_deref()
        .is_some_and(|id| id.eq_ignore_ascii_case("none"))
    {
        return Err(EditorError::Disabled);
    }

    let is_custom = requested_id
        .as_deref()
        .is_some_and(|id| id.eq_ignore_ascii_case("custom"));

    let (raw_command, raw_template) = match requested_id.as_deref() {
        Some(id) if !is_custom => match resolve_preset(id) {
            Some(preset) => (
                String::from(preset.command),
                String::from(preset.args_template),
            ),
            None => (String::from(id), String::from("+%line %file")),
        },
        _ => match req.editor.as_ref() {
            Some(spec) if spec.command.is_some() || spec.args_template.is_some() => (
                spec.command.as_deref().unwrap_or("").trim().to_owned(),
                spec.args_template
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .to_owned(),
            ),
            _ if is_custom => return Err(EditorError::NotConfigured { is_custom: true }),
            _ => {
                let default = &EDITOR_PRESETS[0];
                (
                    String::from(default.command),
                    String::from(default.args_template),
                )
            }
        },
    };

    if raw_command.is_empty() || raw_template.is_empty() {
        return Err(EditorError::NotConfigured { is_custom });
    }

    let command = validate_editor_command(&raw_command)?;
    let parsed_tokens = parse_editor_args_template(&raw_template)?;
    if parsed_tokens.is_empty() {
        return Err(EditorError::NotConfigured { is_custom });
    }

    let file_str = resolved_file_path.to_string_lossy();
    let line_str = req.line.filter(|&n| n > 0).unwrap_or(1).to_string();

    // Replace %line before %file so literal "%line" inside a path is preserved.
    let args = parsed_tokens
        .into_iter()
        .map(|tok| tok.replace("%line", &line_str).replace("%file", &file_str))
        .collect();

    Ok(EditorSpawnSpec { command, args })
}

/// Validates `raw_file_path` stays inside `repo_root` even in the presence of symlinks.
///
/// # Errors
///
/// Returns [`EditorError`] if the path is invalid, virtual (`Commit Message`), or escapes `repo_root`.
pub fn resolve_repo_file_for_editor(
    repo_root: &Path,
    raw_file_path: &str,
) -> Result<PathBuf, EditorError> {
    if raw_file_path.contains('\0') {
        return Err(EditorError::InvalidInput(String::from(
            "filePath cannot contain NUL bytes",
        )));
    }
    let rel = validate_repo_relative_path(raw_file_path)
        .map_err(|err| EditorError::InvalidInput(err.to_string()))?;
    if rel == Path::new(COMMIT_MESSAGE_PATH) {
        return Err(EditorError::InvalidInput(String::from(
            "open in editor is not available for virtual commit message",
        )));
    }

    let canonical_root = repo_root
        .canonicalize()
        .map_err(|err| EditorError::InvalidInput(format_io_context("repository root", err)))?;
    let candidate = canonical_root.join(&rel);

    if candidate.exists() {
        let canonical_file = candidate
            .canonicalize()
            .map_err(|_| EditorError::PathEscapesRepo)?;
        if !canonical_file.starts_with(&canonical_root) {
            return Err(EditorError::PathEscapesRepo);
        }
        return Ok(canonical_file);
    }

    let mut ancestor = candidate.as_path();
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        let Some(name) = ancestor.file_name() else {
            return Err(EditorError::PathEscapesRepo);
        };
        suffix.push(name.to_os_string());
        let Some(parent) = ancestor.parent() else {
            return Err(EditorError::PathEscapesRepo);
        };
        ancestor = parent;
    }

    let canonical_ancestor = ancestor
        .canonicalize()
        .map_err(|_| EditorError::PathEscapesRepo)?;
    if !canonical_ancestor.starts_with(&canonical_root) {
        return Err(EditorError::PathEscapesRepo);
    }

    let mut rebuilt = canonical_ancestor;
    for part in suffix.into_iter().rev() {
        rebuilt.push(part);
    }
    Ok(rebuilt)
}

/// Validates `req` and spawns the configured editor asynchronously without invoking a shell.
///
/// # Errors
///
/// Returns [`EditorError`] if validation fails or the OS cannot spawn the editor binary.
pub fn open_in_editor(
    repo_root: &Path,
    req: &OpenInEditorRequest,
) -> Result<OpenInEditorResponse, EditorError> {
    let resolved_path = resolve_repo_file_for_editor(repo_root, &req.file_path)?;
    let spec = build_spawn_spec(req, &resolved_path)?;

    let mut child = Command::new(&spec.command)
        .args(&spec.args)
        .current_dir(repo_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(false)
        .spawn()
        .map_err(|source| EditorError::SpawnFailed {
            command: spec.command.clone(),
            source,
        })?;

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            let _ = child.wait().await;
        });
    }

    Ok(OpenInEditorResponse { success: true })
}

fn validate_editor_command(raw: &str) -> Result<String, EditorError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(EditorError::InvalidInput(String::from(
            "editor command cannot be empty",
        )));
    }
    if trimmed.starts_with('-') {
        return Err(EditorError::InvalidInput(String::from(
            "editor command cannot start with '-'",
        )));
    }
    if trimmed
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '\0')
    {
        return Err(EditorError::InvalidInput(String::from(
            "editor command must be an executable name or path without spaces",
        )));
    }
    let cmd_path = Path::new(trimmed);
    if cmd_path
        .components()
        .any(|comp| matches!(comp, Component::ParentDir))
    {
        return Err(EditorError::InvalidInput(String::from(
            "editor command cannot contain '..'",
        )));
    }

    let stem = cmd_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(trimmed)
        .to_ascii_lowercase();
    if BLOCKED_INTERPRETER_NAMES.contains(&stem.as_str()) {
        return Err(EditorError::InvalidInput(alloc::format!(
            "refusing to run shell or script interpreter '{stem}' as editor"
        )));
    }

    Ok(String::from(trimmed))
}

fn format_io_context(label: &str, err: impl fmt::Display) -> String {
    alloc::format!("failed to resolve {label}: {err}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tellit_types::EditorSpecRequest;

    #[test]
    fn parses_quoted_and_escaped_editor_args_templates() {
        let parsed =
            parse_editor_args_template(r#"--goto "%file:%line" '--tag=foo\\bar' -w"#).unwrap();
        assert_eq!(
            parsed,
            vec!["--goto", "%file:%line", r"--tag=foo\\bar", "-w"]
        );
        assert!(parse_editor_args_template("--unclosed \"foo").is_err());
    }

    #[test]
    fn blocks_shell_interpreters_and_flag_commands() {
        assert!(validate_editor_command("bash").is_err());
        assert!(validate_editor_command("/bin/sh").is_err());
        assert!(validate_editor_command("-exec").is_err());
        assert!(validate_editor_command("code --wait").is_err());
        assert_eq!(validate_editor_command("code").unwrap(), "code");
    }

    #[test]
    fn build_spawn_spec_substitutes_file_and_line_preserving_single_argv() {
        let req = OpenInEditorRequest {
            file_path: String::from("src/my file.rs"),
            line: Some(42),
            editor: Some(EditorSpecRequest {
                id: Some(String::from("vscode")),
                command: None,
                args_template: None,
            }),
        };
        let spec = build_spawn_spec(&req, Path::new("/repo/src/my file.rs")).unwrap();
        assert_eq!(spec.command, "code");
        assert_eq!(spec.args, vec!["-g", "/repo/src/my file.rs:42"]);

        let percent_spec = build_spawn_spec(&req, Path::new("/repo/src/%line.rs")).unwrap();
        assert_eq!(percent_spec.args, vec!["-g", "/repo/src/%line.rs:42"]);
    }

    #[test]
    fn resolve_repo_file_rejects_traversal_and_virtual_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("src/main.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "fn main() {}\n").unwrap();

        assert!(resolve_repo_file_for_editor(dir.path(), "../escape.rs").is_err());
        assert!(resolve_repo_file_for_editor(dir.path(), "/etc/passwd").is_err());
        assert!(resolve_repo_file_for_editor(dir.path(), COMMIT_MESSAGE_PATH).is_err());
        assert_eq!(
            resolve_repo_file_for_editor(dir.path(), "src/main.rs").unwrap(),
            file.canonicalize().unwrap()
        );
    }
}
