//! Reading Claude Code transcripts (JSON lines), without loading them whole:
//! they grow to tens of megabytes.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const CHUNK: u64 = 64 * 1024;
/// Longer lines are skipped: nothing cc-moss looks for is that long.
const MAX_LINE: usize = 8 << 20;
/// Stop looking after this many bytes from the end. A rename or turn record
/// further back is not found; reading more on every picker refresh costs
/// more than it is worth.
const SCAN_BUDGET: u64 = 32 << 20;

/// The session name from the last `/rename` (a `custom-title` record),
/// searching backwards from the end of the file.
///
/// Claude Code does not always put the name in hook payloads (e.g. after
/// `--resume`), but it always records renames in the transcript.
pub fn last_custom_title(path: &Path) -> Option<String> {
    custom_title_since(path, 0).0
}

/// Like [`last_custom_title`], but only reads bytes from offset `from` on
/// (a size returned by an earlier call). Returns the title, if any, and the
/// size scanned up to, so a caller can resume where it stopped.
pub fn custom_title_since(path: &Path, from: u64) -> (Option<String>, u64) {
    let Ok(mut file) = File::open(path) else {
        return (None, from);
    };
    let Ok(size) = file.metadata().map(|m| m.len()) else {
        return (None, from);
    };
    // A file that shrank was replaced: read it all again.
    let from = if from > size { 0 } else { from };
    (scan_back(&mut file, from, size, custom_title), size)
}

/// Whether Claude Code is in the middle of a turn, judged by the last record
/// that says so: `Some(false)` once the turn ended or the user interrupted
/// it, `Some(true)` while it waits on the model or a tool, `None` when the
/// transcript is missing or says nothing yet.
///
/// Unlike `claude agents`, background shells and agents still running after
/// the turn do not count.
pub fn turn_open(path: &Path) -> Option<bool> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    scan_back(&mut file, 0, size, turn_state)
}

fn turn_state(line: &[u8]) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_slice(line).ok()?;
    let message = v.get("message");
    match v.get("type")?.as_str()? {
        "system" => match v.get("subtype")?.as_str()? {
            "turn_duration" | "stop_hook_summary" => Some(false),
            _ => None,
        },
        "assistant" => {
            let stop = message?.get("stop_reason")?.as_str()?;
            Some(stop != "end_turn")
        }
        // Meta records and local slash commands (`/rename`) do not start a turn.
        "user" if v.get("isMeta").and_then(|m| m.as_bool()) == Some(true) => None,
        "user" => {
            let text = first_text(message?.get("content")?);
            match text {
                Some(t) if t.starts_with("[Request interrupted by user") => Some(false),
                Some(t) if LOCAL_COMMAND.iter().any(|tag| t.starts_with(tag)) => None,
                _ => Some(true),
            }
        }
        "api-request" => Some(true),
        _ => None,
    }
}

const LOCAL_COMMAND: [&str; 3] = ["<command-name>", "<command-message>", "<local-command-"];

fn first_text(content: &serde_json::Value) -> Option<&str> {
    match content {
        serde_json::Value::String(s) => Some(s.as_str()),
        serde_json::Value::Array(parts) => parts
            .iter()
            .find_map(|p| p.get("text").and_then(|t| t.as_str())),
        _ => None,
    }
}

/// Feed lines to `pick` from the last one back to offset `from`, stopping at
/// the first answer.
///
/// Bounded so a huge transcript cannot stall a hook or the picker: a line
/// longer than [`MAX_LINE`] (a pasted image, a large tool result) is skipped
/// without being collected, and the scan gives up after [`SCAN_BUDGET`]
/// bytes. Pieces of a line cut by chunk boundaries are joined once, when
/// the line's start is found, so the cost stays linear in the bytes read.
fn scan_back<T>(
    file: &mut File,
    from: u64,
    size: u64,
    mut pick: impl FnMut(&[u8]) -> Option<T>,
) -> Option<T> {
    scan_back_at(file, from, size, SCAN_BUDGET, |line, _| pick(line))
        .ok()
        .flatten()
}

fn scan_back_at<T>(
    file: &mut File,
    from: u64,
    size: u64,
    budget: u64,
    mut pick: impl FnMut(&[u8], u64) -> Option<T>,
) -> std::io::Result<Option<T>> {
    let mut pos = size;
    // The line that continues into later chunks, as pieces, latest first.
    let mut tail: Vec<Vec<u8>> = Vec::new();
    let mut tail_len = 0usize;
    // The current line is over MAX_LINE: drop it once its start is reached.
    let mut skipping = false;
    while pos > from {
        if size - pos >= budget {
            return Ok(None);
        }
        let start = pos
            .saturating_sub(CHUNK)
            .max(from)
            .max(size.saturating_sub(budget));
        let mut buf = vec![0; (pos - start) as usize];
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut buf)?;
        // `from` is a line boundary; any other chunk start may cut a line.
        let at_line_start = start == from;
        let last_nl = buf.iter().rposition(|b| *b == b'\n');
        if last_nl.is_none() && !at_line_start {
            // The whole chunk is the middle of one line.
            if !skipping {
                tail_len += buf.len();
                tail.push(buf);
                if tail_len > MAX_LINE {
                    tail.clear();
                    tail_len = 0;
                    skipping = true;
                }
            }
            pos = start;
            continue;
        }
        // The line that ends after this chunk starts here.
        let cut = last_nl.map_or(0, |n| n + 1);
        if !skipping {
            let mut line = buf[cut..].to_vec();
            for piece in tail.drain(..).rev() {
                line.extend_from_slice(&piece);
            }
            if line.len() <= MAX_LINE {
                if let Some(answer) = pick(&line, start + cut as u64) {
                    return Ok(Some(answer));
                }
            }
        }
        tail.clear();
        tail_len = 0;
        skipping = false;
        if let Some(last) = last_nl {
            // Whole lines inside the chunk; the piece before the first
            // newline may be cut, so it waits for the next chunk.
            let first = if at_line_start {
                0
            } else {
                let nl = buf.iter().position(|b| *b == b'\n').unwrap_or(last);
                tail_len = nl;
                tail.push(buf[..nl].to_vec());
                nl + 1
            };
            if first <= last {
                let mut end = start + last as u64;
                for line in buf[first..last].split(|b| *b == b'\n').rev() {
                    let offset = end - line.len() as u64;
                    end = offset.saturating_sub(1);
                    if let Some(answer) = pick(line, offset) {
                        return Ok(Some(answer));
                    }
                }
            }
        }
        pos = start;
    }
    Ok(None)
}

fn custom_title(line: &[u8]) -> Option<String> {
    let needle = b"custom-title";
    if !line.windows(needle.len()).any(|w| w == needle) {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(line).ok()?;
    if v.get("type")?.as_str()? != "custom-title" {
        return None;
    }
    v.get("customTitle")?
        .as_str()
        .filter(|t| !t.is_empty())
        .map(String::from)
}

/// The last thing the user typed (not a tool result, meta record, local
/// command or interrupt), and when, in Unix milliseconds; searching back
/// from the end, bounded like the rest of this file.
pub fn last_prompt(path: &Path) -> Option<(String, i64)> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    scan_back(&mut file, 0, size, prompt)
}

fn prompt(line: &[u8]) -> Option<(String, i64)> {
    let needle = b"\"user\"";
    if !line.windows(needle.len()).any(|w| w == needle) {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(line).ok()?;
    // Meta records and the summary a compaction writes are Claude Code's.
    let flag = |k: &str| v.get(k).and_then(|m| m.as_bool()) == Some(true);
    if v.get("type")?.as_str()? != "user" || flag("isMeta") || flag("isCompactSummary") {
        return None;
    }
    let content = v.get("message")?.get("content")?;
    let tool_result = content.as_array().is_some_and(|parts| {
        parts
            .iter()
            .any(|p| p.get("type").and_then(|t| t.as_str()) == Some("tool_result"))
    });
    if tool_result {
        return None;
    }
    let text = first_text(content)?.trim();
    if text.is_empty()
        || text.starts_with("[Request interrupted")
        || LOCAL_COMMAND.iter().any(|tag| text.starts_with(tag))
        || crate::slot::is_notice(text)
    {
        return None;
    }
    let at = time::OffsetDateTime::parse(
        v.get("timestamp")?.as_str()?,
        &time::format_description::well_known::Rfc3339,
    )
    .ok()?;
    let ms = i64::try_from(at.unix_timestamp_nanos() / 1_000_000).ok()?;
    Some((text.to_string(), ms))
}

/// 最近一条可信主会话响应使用的模型，仅在恢复时读取。
pub fn last_assistant_model(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    scan_back(&mut file, 0, size, |line| {
        let v: serde_json::Value = serde_json::from_slice(line).ok()?;
        if !trusted_assistant(&v)
            || v.get("isApiErrorMessage").and_then(|v| v.as_bool()) == Some(true)
        {
            return None;
        }
        safe_model(v.get("message")?.get("model")?.as_str()?)
    })
}

fn trusted_assistant(v: &serde_json::Value) -> bool {
    v.get("type").and_then(|v| v.as_str()) == Some("assistant")
        && !["isSynthetic", "isSidechain"]
            .iter()
            .any(|key| v.get(key).and_then(|v| v.as_bool()) == Some(true))
}

fn safe_model(model: &str) -> Option<String> {
    crate::provider::safe_model(model).then(|| model.to_owned())
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiFailureKind {
    ModelUnavailable,
    GenericApi,
}

/// 仅允许持久化的安全错误元数据，不包含错误正文。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ApiFailure {
    pub kind: ApiFailureKind,
    pub status: Option<i64>,
    pub model: Option<String>,
    pub at_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct RequestResult {
    pub position: u64,
    pub failure: Option<ApiFailure>,
    pub success: Option<ModelResponse>,
    pub evidence: Option<ModelEvidence>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelResponse {
    pub model: String,
    pub at_ms: Option<i64>,
}

/// 仅证明某配置文件版本之后目标模型完成过响应，不证明endpoint或认证。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelEvidence {
    pub session_id: String,
    pub path: String,
    pub generation: String,
    pub epoch: String,
    pub fence_ms: i64,
    pub position: u64,
    pub model: String,
    pub at_ms: i64,
}

/// 返回最新完整结果及完整行边界。读取失败不推进游标。
pub fn request_result_since(
    path: &Path,
    from: u64,
) -> std::io::Result<(Option<RequestResult>, u64)> {
    request_result_for_session(path, from, None)
}

/// 有sessionId的行必须属于当前会话，不能继承clear前的记录。
pub fn request_result_for_session(
    path: &Path,
    from: u64,
    session_id: Option<&str>,
) -> std::io::Result<(Option<RequestResult>, u64)> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let from = if from > size { 0 } else { from };
    let mut end = size;
    let (complete, spent) = loop {
        if end <= from || size - end >= SCAN_BUDGET {
            return Ok((None, from));
        }
        let start = end.saturating_sub(CHUNK).max(from);
        let mut bytes = vec![0; (end - start) as usize];
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut bytes)?;
        if let Some(n) = bytes.iter().rposition(|b| *b == b'\n') {
            break (start + n as u64 + 1, size - start);
        }
        end = start;
    };
    let result = scan_back_at(
        &mut file,
        from,
        complete,
        SCAN_BUDGET.saturating_sub(spent),
        |line, position| {
            request_result(line, session_id).map(|mut result| {
                result.position = position;
                result
            })
        },
    )?;
    Ok((result, complete))
}

fn request_result(line: &[u8], session_id: Option<&str>) -> Option<RequestResult> {
    let v: serde_json::Value = serde_json::from_slice(line).ok()?;
    if !trusted_assistant(&v)
        || session_id.is_some_and(|expected| {
            v.get("sessionId")
                .and_then(|v| v.as_str())
                .is_some_and(|actual| actual != expected)
        })
    {
        return None;
    }
    let at_ms = v
        .get("timestamp")
        .and_then(|v| v.as_str())
        .and_then(|s| {
            time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
        })
        .and_then(|at| i64::try_from(at.unix_timestamp_nanos() / 1_000_000).ok());
    let message = v.get("message")?;
    if v.get("isApiErrorMessage").and_then(|v| v.as_bool()) != Some(true) {
        // 本地合成响应和没有模型身份的记录，不是可信成功。
        let model = safe_model(message.get("model")?.as_str()?)?;
        let stop = message.get("stop_reason")?.as_str()?;
        if !matches!(
            stop,
            "end_turn"
                | "tool_use"
                | "max_tokens"
                | "stop_sequence"
                | "pause_turn"
                | "refusal"
                | "model_context_window_exceeded"
        ) {
            return None;
        }
        return Some(RequestResult {
            position: 0,
            failure: None,
            success: Some(ModelResponse { model, at_ms }),
            evidence: None,
        });
    }
    let text = message.get("content").and_then(first_text).unwrap_or("");
    let lower = text.to_ascii_lowercase();
    let kind = if lower.contains("unknown provider for model") {
        ApiFailureKind::ModelUnavailable
    } else {
        ApiFailureKind::GenericApi
    };
    let status = v
        .get("status")
        .and_then(|v| v.as_i64())
        .or_else(|| v.get("error")?.get("status")?.as_i64())
        .filter(|n| (100..=599).contains(n))
        .or_else(|| {
            lower
                .strip_prefix("api error:")?
                .split_whitespace()
                .next()?
                .parse::<i64>()
                .ok()
                .filter(|n| (100..=599).contains(n))
        });
    let model = message
        .get("model")
        .and_then(|v| v.as_str())
        .and_then(safe_model);
    Some(RequestResult {
        position: 0,
        failure: Some(ApiFailure {
            kind,
            status,
            model,
            at_ms,
        }),
        success: None,
        evidence: None,
    })
}

/// 包含inode及纳秒时间；相同长度替换也会触发重核。
pub fn request_file_stamp(path: &Path) -> std::io::Result<String> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(path)?;
    Ok(format!(
        "{}:{}:{}:{}:{}:{}:{}",
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec()
    ))
}
