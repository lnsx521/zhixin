//! 文件互传：手机浏览器 ↔ 电脑，双向。
//!
//! 挂在 HTTP 接入的同一端口上（需在网络接入页开启接收器）。
//! 手机扫码或输入 `http://<电脑>:<端口>/transfer?t=<令牌>`：
//! - 上传：页面上选文件 → POST multipart → 落进接收文件夹；
//! - 下载：页面列出接收文件夹里的文件，点击即下载。
//!
//! 无需任何客户端与快捷指令，Safari 直接用。
//!
//! 安全：与通知接口同一令牌鉴权；仅允许同源浏览器的跨站防护
//! （Origin 必须与 Host 一致，导航与 curl 无 Origin 不受限）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path as AxumPath, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// 单个上传体积上限（手机传视频也够用）。
const MAX_BODY: usize = 512 * 1024 * 1024;
/// 列表最多返回的条目数。
const LIST_LIMIT: usize = 300;
/// 文件名长度上限。
const NAME_MAX: usize = 200;

// ── 文件夹 ────────────────────────────────

/// 接收文件夹：优先用用户配置，否则默认「下载/知信」。
pub fn folder(app: &AppHandle) -> PathBuf {
    let configured = app.state::<AppState>().config().transfer.folder.clone();
    match configured {
        Some(f) if !f.trim().is_empty() => PathBuf::from(f.trim()),
        _ => dirs::download_dir()
            .unwrap_or_else(|| crate::paths::data_dir(app).to_path_buf())
            .join("知信"),
    }
}

/// 文件名只允许是「纯文件名」：拒绝路径分隔符、上跳、控制字符。
fn sanitize_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.len() > NAME_MAX
        || trimmed.contains(['/', '\\'])
        || trimmed.contains("..")
        || trimmed.chars().any(|c| c.is_control())
    {
        return None;
    }
    Some(trimmed.to_string())
}

/// 重名时在扩展名前追加「 (n)」，不覆盖已有文件。
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(name);
    let ext = Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    for n in 1..u32::MAX {
        let next = dir.join(format!("{stem} ({n}){ext}"));
        if !next.exists() {
            return next;
        }
    }
    candidate
}

// ── 鉴权与同源 ────────────────────────────

fn reply(code: StatusCode, status: &str) -> Response {
    (
        code,
        Json(serde_json::json!({ "ok": code.is_success(), "status": status })),
    )
        .into_response()
}

/// Origin 存在时必须与 Host 同源（拦跨站表单），令牌必须正确。
#[expect(
    clippy::result_large_err,
    reason = "统一用 axum Response 直接返回，无需装箱"
)]
fn guard(app: &AppHandle, headers: &HeaderMap, token: Option<&str>) -> Result<(), Response> {
    if let Some(origin) = headers.get("origin").and_then(|h| h.to_str().ok()) {
        let host = headers
            .get("host")
            .and_then(|h| h.to_str().ok())
            .unwrap_or_default();
        if origin != format!("http://{host}") {
            return Err(reply(StatusCode::FORBIDDEN, "cross_origin"));
        }
    }
    let Some(token) = token else {
        return Err(reply(StatusCode::UNAUTHORIZED, "missing_token"));
    };
    match app
        .state::<crate::http_ingress::HttpIngress>()
        .running_endpoint()
    {
        Some((_, _, Some(real))) if real == token => Ok(()),
        _ => Err(reply(StatusCode::UNAUTHORIZED, "unauthorized")),
    }
}

fn query_token(params: &HashMap<String, String>) -> Option<String> {
    params
        .get("t")
        .or_else(|| params.get("token"))
        .filter(|s| !s.is_empty())
        .cloned()
}

// ── 路由处理 ──────────────────────────────

async fn page(
    State(app): State<AppHandle>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if let Err(response) = guard(&app, &headers, query_token(&params).as_deref()) {
        return response;
    }
    Html(PAGE_HTML).into_response()
}

async fn list(
    State(app): State<AppHandle>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if let Err(response) = guard(&app, &headers, query_token(&params).as_deref()) {
        return response;
    }
    let dir = folder(&app);
    let _ = std::fs::create_dir_all(&dir);
    // 目录导航：dir 参数指定当前浏览的子目录（默认根目录），返回其直接子文件夹与文件
    let current = params.get("dir").and_then(|d| sanitize_rel(d)).unwrap_or_default();
    let browse_dir = if current.is_empty() { dir.clone() } else { dir.join(&current) };
    if let Err(_) = tokio::fs::metadata(&browse_dir).await {
        return reply(StatusCode::NOT_FOUND, "dir_not_found");
    }
    let mut folders: Vec<serde_json::Value> = Vec::new();
    let mut entries: Vec<FileEntry> = Vec::new();
    if let Ok(mut read) = tokio::fs::read_dir(&browse_dir).await {
        while let Ok(Some(entry)) = read.next_entry().await {
            let Ok(meta) = entry.metadata().await else { continue };
            let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
            if meta.is_dir() {
                let sub = browse_dir.join(&name);
                let mut folder_size: u64 = 0;
                let mut latest: u64 = 0;
                for entry in walkdir(&sub) {
                    if let Ok(fm) = entry.metadata() {
                        folder_size += fm.len();
                        if let Ok(modified) = fm.modified() {
                            if let Ok(d) = modified.duration_since(std::time::UNIX_EPOCH) {
                                latest = latest.max(d.as_secs());
                            }
                        }
                    }
                }
                folders.push(serde_json::json!({
                    "name": name,
                    "size": folder_size,
                    "mtime": latest,
                }));
            } else {
                entries.push(FileEntry {
                    name,
                    size: meta.len(),
                    mtime: meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                });
            }
        }
    }
    folders.sort_by_key(|f| f.get("mtime").and_then(|m| m.as_u64()).unwrap_or(0));
    folders.reverse();
    entries.sort_by_key(|a| std::cmp::Reverse(a.mtime));
    entries.truncate(LIST_LIMIT);
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "ok": true, "dir": current, "folders": folders, "files": entries })),
    )
        .into_response()
}

/// 递归收集接收文件夹内的文件（相对路径，最深 3 层）。
fn collect_files<'a>(
    dir: &'a std::path::Path,
    rel: &'a str,
    entries: &'a mut Vec<FileEntry>,
    depth: u32,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        if depth > 3 {
            return;
        }
        if let Ok(mut read) = tokio::fs::read_dir(dir).await {
            while let Ok(Some(entry)) = read.next_entry().await {
                let Ok(meta) = entry.metadata().await else { continue };
                let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
                let child_rel = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
                if meta.is_dir() {
                    Box::pin(collect_files(&dir.join(&name), &child_rel, entries, depth + 1)).await;
                } else {
                    entries.push(FileEntry {
                        name: child_rel,
                        size: meta.len(),
                        mtime: meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs())
                            .unwrap_or(0),
                    });
                }
            }
        }
    })
}

async fn upload(
    State(app): State<AppHandle>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    mut multipart: Multipart,
) -> Response {
    if let Err(response) = guard(&app, &headers, query_token(&params).as_deref()) {
        return response;
    }
    let root = folder(&app);
    if let Err(e) = std::fs::create_dir_all(&root) {
        log::warn!("文件互传：创建接收文件夹失败：{e}");
        return reply(StatusCode::INTERNAL_SERVER_ERROR, "cannot_create_folder");
    }
    // 上传目标 = 手机当前浏览的子目录（无则根目录）；子目录须已存在
    let current = params.get("dir").and_then(|d| sanitize_rel(d)).unwrap_or_default();
    let dir = if current.is_empty() { root.clone() } else { root.join(&current) };
    if let Err(_) = tokio::fs::metadata(&dir).await {
        return reply(StatusCode::NOT_FOUND, "dir_not_found");
    }
    let mut saved: Vec<String> = Vec::new();
    let mut shared_text: Option<String> = None;
    while let Ok(Some(mut field)) = multipart.next_field().await {
        let Some(raw_name) = field.file_name().map(str::to_string) else {
            // 无文件名的字段 = 快捷指令传来的共享文本（任意字段名都收）
            if let Ok(text) = field.text().await {
                let trimmed = text.trim().to_string();
                if !trimmed.is_empty() {
                    shared_text = Some(trimmed);
                }
            }
            continue;
        };
        let Some(name) = sanitize_name(&raw_name) else {
            continue;
        };
        // 一律平铺在接收文件夹根目录（老板定：不按类型/日期建子文件夹）
        let target = unique_path(&dir, &name);
        let file = match tokio::fs::File::create(&target).await {
            Ok(f) => f,
            Err(e) => {
                log::warn!("文件互传：无法写入 {target:?}：{e}");
                return reply(StatusCode::INTERNAL_SERVER_ERROR, "write_failed");
            }
        };
        let mut writer = tokio::io::BufWriter::new(file);
        let mut size: u64 = 0;
        let mut failed = false;
        while let Ok(Some(chunk)) = field.chunk().await {
            size += chunk.len() as u64;
            if size > MAX_BODY as u64 {
                failed = true;
                break;
            }
            if tokio::io::AsyncWriteExt::write_all(&mut writer, &chunk)
                .await
                .is_err()
            {
                failed = true;
                break;
            }
        }
        if failed {
            let _ = tokio::fs::remove_file(&target).await;
            return reply(StatusCode::PAYLOAD_TOO_LARGE, "file_too_large");
        }
        if tokio::io::AsyncWriteExt::flush(&mut writer).await.is_err() {
            let _ = tokio::fs::remove_file(&target).await;
            return reply(StatusCode::INTERNAL_SERVER_ERROR, "write_failed");
        }
        log::info!("文件互传：收到 {name}（{size} 字节）→ {target:?}");
        saved.push(
            target
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&name)
                .to_string(),
        );
    }
    if saved.is_empty() {
        if let Some(text) = shared_text.filter(|t| !t.trim().is_empty()) {
            let trimmed = text.trim();
            let stamp = share_stamp();
            let is_url = trimmed.starts_with("http://") || trimmed.starts_with("https://");
            if is_url {
                // 网页/链接：抓网页标题命名，生成 Windows 快捷方式（双击直接打开网页）
                let base = match fetch_page_title(trimmed).await {
                    Some(title) if !title.trim().is_empty() => sanitize_title(&title),
                    _ => sanitize_title(&domain_of(trimmed)),
                };
                let fallback = sanitize_title(&domain_of(trimmed));
                let name = if base.is_empty() { fallback } else { base };
                let target = unique_path(&dir, &format!("{name}-{stamp}.url"));
                match std::fs::write(&target, format!("[InternetShortcut]\r\nURL={}", trimmed)) {
                    Ok(()) => {
                        log::info!("文件互传：收到网页链接 → {target:?}");
                        saved.push(
                            target
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("link")
                                .to_string(),
                        );
                    }
                    Err(e) => {
                        log::warn!("文件互传：写入链接快捷方式失败：{e}");
                        return reply(StatusCode::INTERNAL_SERVER_ERROR, "write_failed");
                    }
                }
            } else {
                // 纯文本：存 txt
                let target = unique_path(&dir, &format!("文本-{stamp}.txt"));
                match std::fs::write(&target, trimmed) {
                    Ok(()) => {
                        log::info!("文件互传：收到共享文本 → {target:?}");
                        saved.push(
                            target
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("text")
                                .to_string(),
                        );
                    }
                    Err(e) => {
                        log::warn!("文件互传：写入共享文本失败：{e}");
                        return reply(StatusCode::INTERNAL_SERVER_ERROR, "write_failed");
                    }
                }
            }
        }
    }
    Json(serde_json::json!({ "ok": true, "saved": saved })).into_response()
}

/// 按扩展名推断 MIME 类型。
fn mime_by_ext(name: &str) -> String {
    let lower = name.to_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "bmp" => "image/bmp",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "pdf" => "application/pdf",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// 解析 Range 头（bytes=start-end / bytes=start-）。
fn parse_range(range: &str, total: u64) -> Option<(u64, u64)> {
    let spec = range.strip_prefix("bytes=")?;
    let (start_s, end_s) = spec.split_once('-')?;
    let start: u64 = start_s.trim().parse().ok()?;
    let end: u64 = match end_s.trim().parse::<u64>() {
        Ok(value) => value.min(total.saturating_sub(1)),
        Err(_) => total.saturating_sub(1),
    };
    if start > end || start >= total {
        return None;
    }
    Some((start, end))
}

/// 相对路径清洗：允许子目录，拒绝穿越。
fn sanitize_rel(rel: &str) -> Option<String> {
    let trimmed = rel.trim();
    if trimmed.is_empty()
        || trimmed.len() > 400
        || trimmed.starts_with('/')
        || trimmed.starts_with('\\')
        || trimmed.contains("..")
        || trimmed.split('/').any(|part| part.contains('\\') || part.is_empty())
        || trimmed.chars().any(|c| c.is_control())
    {
        return None;
    }
    Some(trimmed.to_string())
}

/// 手机端删除电脑文件（带令牌校验 + 路径防穿越）。
/// 抓取网页标题（3 秒超时；失败返回 None——外网/被墙站点走域名兜底）。
async fn fetch_page_title(url: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;
    let body = client.get(url).send().await.ok()?.text().await.ok()?;
    let lower = body.to_lowercase();
    let start = lower.find("<title")?;
    let after_gt = body[start..].find('>')? + start + 1;
    let rest = &body[after_gt..];
    let end = rest.to_lowercase().find("</title")?;
    let raw = rest[..end].trim();
    if raw.is_empty() {
        return None;
    }
    Some(decode_entities(raw))
}

/// 基础 HTML 实体还原。
fn decode_entities(raw: &str) -> String {
    raw.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// 文件名清洗：去非法字符、压空格、限长（按字符数）。
fn sanitize_title(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\n' | '\r'))
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = collapsed.chars().collect();
    if chars.len() > 40 {
        String::from_iter(&chars[..40])
    } else {
        collapsed
    }
}

/// 从 URL 提取域名（去 www 前缀）。
fn domain_of(url: &str) -> String {
    let after = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let host = after.split('/').next().unwrap_or("");
    host.trim_start_matches("www.").split(':').next().unwrap_or(host).to_string()
}

/// 同步递归遍历目录下的全部文件（统计文件夹大小用）。
fn walkdir(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    let mut depth = 0;
    while let Some(cur) = stack.pop() {
        depth += 1;
        if depth > 6 {
            continue;
        }
        if let Ok(read) = std::fs::read_dir(&cur) {
            for entry in read.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    out.push(path);
                }
            }
        }
    }
    out
}

/// 共享文本落盘文件名时间戳（东八区，YYYYMMDD-HHMMSS）。
fn share_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        + 8 * 3600;
    let days = secs / 86400;
    let rem = secs % 86400;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let h = rem / 3600;
    let mi = (rem % 3600) / 60;
    let sec = rem % 60;
    format!("{y:04}{m:02}{d:02}-{h:02}{mi:02}{sec:02}")
}

async fn delete_file(
    State(app): State<AppHandle>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if let Err(response) = guard(&app, &headers, query_token(&params).as_deref()) {
        return response;
    }
    let Some(raw) = params.get("name") else {
        return reply(StatusCode::BAD_REQUEST, "invalid_name");
    };
    let Some(name) = sanitize_rel(raw) else {
        return reply(StatusCode::BAD_REQUEST, "invalid_name");
    };
    let dir = folder(&app);
    let path = dir.join(&name);
    let Ok(canon_dir) = dir.canonicalize() else {
        return reply(StatusCode::INTERNAL_SERVER_ERROR, "folder_error");
    };
    match path.canonicalize() {
        Ok(canon) if canon.starts_with(&canon_dir) && canon.is_file() => {
            match tokio::fs::remove_file(&canon).await {
                Ok(()) => ([(header::CONTENT_TYPE, "application/json")], Json(serde_json::json!({"ok": true}))).into_response(),
                Err(e) => reply(StatusCode::INTERNAL_SERVER_ERROR, &format!("delete_failed:{e}")),
            }
        }
        _ => reply(StatusCode::NOT_FOUND, "not_found"),
    }
}

async fn download(
    State(app): State<AppHandle>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if let Err(response) = guard(&app, &headers, query_token(&params).as_deref()) {
        return response;
    }
    let Some(raw) = params.get("name") else {
        return reply(StatusCode::BAD_REQUEST, "invalid_name");
    };
    let Some(name) = sanitize_rel(raw) else {
        return reply(StatusCode::BAD_REQUEST, "invalid_name");
    };
    let inline = params.get("mode").map(|m| m == "view").unwrap_or(false);
    let mime = mime_by_ext(&name);
    let path = folder(&app).join(&name);
    let meta = match tokio::fs::metadata(&path).await {
        Ok(m) if m.is_file() => m,
        _ => return reply(StatusCode::NOT_FOUND, "not_found"),
    };
    let total = meta.len();

    // 视频拖进度条：处理 Range 请求（206 分段流式）
    let range_header = headers.get(header::RANGE).and_then(|v| v.to_str().ok()).map(str::to_string);
    if let Some(range) = range_header.filter(|_| !inline || mime.starts_with("video/") || mime.starts_with("audio/")) {
        if let Some((start, end)) = parse_range(&range, total) {
            let file = match tokio::fs::File::open(&path).await {
                Ok(f) => f,
                Err(_) => return reply(StatusCode::INTERNAL_SERVER_ERROR, "open_failed"),
            };
            use tokio::io::{AsyncSeekExt, AsyncReadExt};
            let mut f = std::pin::pin!(file);
            let _ = f.seek(std::io::SeekFrom::Start(start)).await;
            let length = (end - start + 1).min(total - start);
            let mut buffer = vec![0u8; length as usize];
            if f.read_exact(&mut buffer).await.is_err() {
                return reply(StatusCode::INTERNAL_SERVER_ERROR, "read_failed");
            }
            let mut headers_out = HeaderMap::new();
            headers_out.insert(header::CONTENT_TYPE, HeaderValue::from_str(&mime).unwrap_or(HeaderValue::from_static("application/octet-stream")));
            headers_out.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
            headers_out.insert(header::CONTENT_RANGE, HeaderValue::from_str(&format!("bytes {start}-{end}/{total}")).unwrap_or(HeaderValue::from_static("bytes */*")));
            headers_out.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
            if inline {
                headers_out.insert(header::CONTENT_DISPOSITION, HeaderValue::from_static("inline"));
            }
            return (StatusCode::PARTIAL_CONTENT, headers_out, buffer).into_response();
        }
    }

    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => return reply(StatusCode::NOT_FOUND, "not_found"),
    };
    let length = file.metadata().await.map(|m| m.len()).unwrap_or(0);
    let encoded = utf8_percent_encode(&name, NON_ALPHANUMERIC_KEPT);
    let disposition = if inline { "inline" } else { "attachment" };
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(
            header::CONTENT_DISPOSITION,
            format!("{disposition}; filename*=UTF-8''{encoded}"),
        )
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from_stream(tokio_util::io::ReaderStream::new(file)))
        .unwrap();
    if let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    response
}

// percent-encode：手写一个极小的，避免新依赖。
// 保留字母数字与常见安全字符，其余按 UTF-8 百分号编码。
const NON_ALPHANUMERIC_KEPT: &str =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

fn utf8_percent_encode(input: &str, kept: &str) -> String {
    let mut out = String::new();
    for &byte in input.as_bytes() {
        let ch = byte as char;
        if kept.contains(ch) {
            out.push(ch);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[derive(Serialize, Clone)]
struct FileEntry {
    name: String,
    size: u64,
    mtime: u64,
}

// ── 设置命令 ──────────────────────────────

#[derive(Serialize)]
pub struct TransferInfo {
    pub enabled: bool,
    pub port: u16,
    pub token: Option<String>,
    pub folder: String,
}

fn main_only(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("Settings window required".into())
    }
}

#[tauri::command]
pub fn get_transfer_info(
    window: tauri::WebviewWindow,
    app: AppHandle,
) -> Result<TransferInfo, String> {
    main_only(&window)?;
    let endpoint = app
        .state::<crate::http_ingress::HttpIngress>()
        .running_endpoint();
    let (enabled, port, token) = match endpoint {
        Some((_, port, token)) => (true, port, token),
        None => (false, 0, None),
    };
    Ok(TransferInfo {
        enabled,
        port,
        token,
        folder: folder(&app).to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub fn set_transfer_folder(
    window: tauri::WebviewWindow,
    app: AppHandle,
    folder: String,
) -> Result<String, String> {
    main_only(&window)?;
    let trimmed = folder.trim().to_string();
    if trimmed.is_empty() {
        return Err("Folder cannot be empty".into());
    }
    let path = PathBuf::from(&trimmed);
    std::fs::create_dir_all(&path).map_err(|e| format!("Cannot create folder: {e}"))?;
    let mut config = app.state::<AppState>().config();
    config.transfer.folder = Some(trimmed);
    crate::commands::save_config(app.clone(), config)?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn open_transfer_folder(window: tauri::WebviewWindow, app: AppHandle) -> Result<(), String> {
    main_only(&window)?;
    let dir = folder(&app);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create folder: {e}"))?;
    std::process::Command::new("explorer.exe")
        .arg(&dir)
        .spawn()
        .map_err(|e| format!("Cannot open folder: {e}"))?;
    Ok(())
}

// ── 路由组装 ──────────────────────────────

pub fn router(app: AppHandle) -> Router {
    Router::new()
        .route("/transfer", get(page))
        .route("/transfer/upload", post(upload))
        .route("/transfer/list", get(list))
        .route("/transfer/file", get(download))
        .route("/transfer/delete", post(delete_file))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(app)
}

// ── 手机页面 ──────────────────────────────

const PAGE_HTML: &str = r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>知信 · 文件互传</title>
<style>
  :root { color-scheme: light dark; }
  * { box-sizing: border-box; -webkit-tap-highlight-color: transparent; }
  body {
    margin: 0; padding: 24px 18px 48px; max-width: 560px; margin-inline: auto;
    font-family: -apple-system, "SF Pro Text", "PingFang SC", "Microsoft YaHei", sans-serif;
    background: #f5f5f7; color: #1d1d1f; font-size: 15px; line-height: 1.6;
  }
  .card {
    background: #ffffff; border: 1px solid #e5e5ea; border-radius: 14px;
    padding: 18px; margin-bottom: 16px;
    box-shadow: 0 1px 2px rgba(0,0,0,.04), 0 8px 24px rgba(0,0,0,.06);
  }
  h1 { font-size: 20px; margin: 0 0 2px; letter-spacing: -.3px; }
  .muted { color: #6e6e73; font-size: 12.5px; }
  .btn {
    display: block; width: 100%; border: 0; border-radius: 12px; padding: 13px 0;
    background: #3478f6; color: #fff; font-size: 15.5px; font-weight: 600; cursor: pointer;
  }
  .btn:disabled { opacity: .45; }
  .btn.secondary { background: #e9f1fe; color: #3478f6; margin-top: 10px; }
  input[type=file] { display: none; }
  .row { display: flex; align-items: stretch; gap: 12px; border-bottom: 1px solid rgba(0,0,0,.06); position: relative; overflow: hidden; }
  .row:last-child { border-bottom: 0; }
  .file-name { flex: 1; min-width: 0; overflow-wrap: anywhere; font-size: 14.5px; }
  .file-thumb {
    width: 44px; height: 44px; flex-shrink: 0; border-radius: 9px; overflow: hidden;
    background: rgba(0,0,0,.05);
  }
  .file-thumb img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .thumb-icon {
    display: grid; place-items: center; width: 100%; height: 100%;
    font-size: 18px; color: #6e6e73;
  }
  .thumb-doc { font-size: 10px; font-weight: 700; letter-spacing: .5px; }
  .folder-row {
    display: flex; align-items: center; gap: 12px;
    padding: 13px 0; font-size: 14.5px; font-weight: 600; color: #3478f6;
    cursor: pointer; border-bottom: 1px solid rgba(0,0,0,.06);
  }
  .folder-row:active { opacity: .6; }
  .row { position: relative; overflow: hidden; }
  .row-swipe {
    position: relative; z-index: 1; display: flex; align-items: center; gap: 12px;
    flex: 1; min-width: 0; background: #ffffff; padding: 13px 0;
    transition: transform .18s ease-out;
    will-change: transform;
  }
  /* 滑动期间暂时关掉毛玻璃：backdrop-filter 实时重算是不丝滑的根源 */
  .card.swiping { backdrop-filter: none !important; -webkit-backdrop-filter: none !important; }
  .card.swiping .row-swipe { background: #ffffff; }
  .swipe-del {
    position: absolute; right: 6px; top: 6px; bottom: 6px; width: 60px;
    border: 0; border-radius: 10px; background: #c0392b; color: #fff;
    font-size: 13px; font-weight: 600; cursor: pointer;
  }
  .file-n { font-weight: 500; }
  .file-ext {
    display: inline-block; margin-left: 6px; padding: 1px 6px; border-radius: 5px;
    background: rgba(0,0,0,.06); color: #6e6e73; font-size: 10px; font-weight: 600;
    vertical-align: middle; letter-spacing: .5px;
  }
  .file-meta { color: #6e6e73; font-size: 11.5px; margin-top: 2px; }
  .file-get {
    flex-shrink: 0; border: 0; border-radius: 9px; padding: 8px 14px;
    background: #e9f1fe; color: #3478f6; font-size: 13px; font-weight: 600;
    text-decoration: none; display: inline-block;
  }
  .empty { text-align: center; color: #6e6e73; padding: 18px 0 6px; font-size: 13.5px; }
  .file-sub { display: block; font-size: 10.5px; color: #a6a6ab; }
  .file-ops { display: flex; flex-direction: column; gap: 10px; flex-shrink: 0; align-items: stretch; }
  .file-del {
    border: 0; border-radius: 9px; padding: 6px 12px;
    background: #fdeaea; color: #c0392b; font-size: 12px; font-weight: 600; cursor: pointer;
  }

  .toast {
    position: fixed; left: 50%; bottom: 28px; transform: translateX(-50%) translateY(12px);
    background: rgba(29,29,31,.92); color: #fff; border-radius: 999px; padding: 9px 20px;
    font-size: 13px; opacity: 0; transition: all .25s; pointer-events: none; max-width: 86vw;
  }
  .toast.show { opacity: 1; transform: translateX(-50%) translateY(0); }
  #preview-overlay {
    position: fixed; inset: 0; background: rgba(0,0,0,.92);
    display: flex; flex-direction: column; z-index: 100;
  }
  #preview-topbar {
    display: flex; align-items: center; justify-content: space-between;
    padding: 10px 14px; flex-shrink: 0;
  }
  .preview-nav {
    border: 0; border-radius: 999px; width: 34px; height: 34px;
    background: rgba(255,255,255,.14); color: #fff; font-size: 16px;
    cursor: pointer; display: grid; place-items: center;
  }
  #preview-name { color: #f2f2f4; font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 70vw; }
  #preview-close {
    border: 0; border-radius: 999px; padding: 8px 18px;
    background: rgba(255,255,255,.14); color: #fff; font-size: 14px; font-weight: 600; cursor: pointer;
  }
  #preview-body { flex: 1; min-height: 0; display: flex; align-items: center; justify-content: center; padding: 0 10px 10px; }

  #fab {
    position: fixed; right: 16px; bottom: 20px; width: 66px; height: 66px;
    border: 0; border-radius: 50%; background: #3478f6; color: #fff;
    font-size: 28px; line-height: 60px; text-align: center; cursor: pointer;
    box-shadow: 0 10px 28px rgba(52,120,246,.55), 0 2px 6px rgba(0,0,0,.18);
    z-index: 60; padding: 0; transition: transform .15s, box-shadow .15s;
  }
  #fab:active { transform: scale(0.92); }
  .help-toggle {
    border: 0; background: transparent; color: #3478f6; font-size: 12px;
    font-weight: 600; cursor: pointer; padding: 2px 4px;
  }
  @media (prefers-color-scheme: dark) {
    body { background: #1e1e20; color: #f2f2f4; }
    .card { background: #2a2a2d; border-color: #3a3a3e; }
    .muted { color: #a6a6ab; }
    .file-meta { color: #a6a6ab; }
    .file-ext { background: rgba(255,255,255,.10); color: #a6a6ab; }
    .file-thumb { background: rgba(255,255,255,.08); }
    .thumb-icon { color: #a6a6ab; }
    .empty { color: #a6a6ab; }
    .swipe-del { background: #c0392b; }
    .row-swipe { background: #2a2a2d; padding: 13px 0; }
    .card.swiping .row-swipe { background: #2a2a2d; }
    .folder-row { border-bottom-color: rgba(255,255,255,.09); color: #8ab4ff; }
    .row { border-bottom-color: rgba(255,255,255,.09); }
    .btn.secondary { background: #2c3d5c; color: #8ab4ff; }
    .file-get { background: #2c3d5c; color: #8ab4ff; }
    .toast { background: rgba(60,60,64,.96); }
    .help-toggle { color: #8ab4ff; }
  }
</style>
</head>
<body>
  <div class="card">
    <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:10px">
      <span style="font-size:12px;color:#6e6e73">知信 · 文件互传</span>
      <button class="help-toggle" id="help-toggle">ⓘ 帮助</button>
    </div>
    <input type="file" id="files" multiple>
    <div id="help-box" style="display:none">
      <p class="muted" style="margin:10px 0 6px">配到 iPhone「分享」菜单（可选）</p>
      <p style="font-size:13px;line-height:1.7;margin:0 0 8px">在快捷指令 App 新建指令：</p>
      <p style="font-size:13px;line-height:1.7;margin:0 0 8px"><b>①</b> 添加「从共享表单接收」，类型勾选「图像」「文件」和「文本」；</p>
      <p style="font-size:13px;line-height:1.7;margin:0 0 8px"><b>②</b> 添加「获取 URL 内容」：方法选 POST，URL 填**下方按钮复制的地址**，请求体选「表单」，添加新字段时类型选「文件」，Key 手动输入 file，值选「快捷指令输入」；</p>
      <p style="font-size:13px;line-height:1.7;margin:0 0 10px"><b>③</b> 命名保存。以后照片/文件点「分享」→ 选它，直接传到这台电脑。</p>
      <button class="btn secondary" id="copyurl">复制上传地址</button>
    </div>
    <p class="muted" style="margin:12px 0 4px">电脑上的文件</p>
    <div id="list"></div>
  </div>
  </div>
  <div class="card" id="progress-wrap" style="display:none">
    <div style="height:8px;border-radius:6px;background:#e2e0d8;overflow:hidden">
      <div id="progress-fill" style="height:100%;width:0%;background:#3b82d6;border-radius:6px;transition:width .2s"></div>
    </div>
    <p id="progress-text" style="font-size:12px;color:#5f5e5a;margin:6px 0 0">上传中…</p>
  </div>
  <button id="fab" aria-label="上传">+</button>
  <div id="preview-overlay" style="display:none">
    <div id="preview-topbar">
      <button class="preview-nav" id="preview-prev" aria-label="上一张">‹</button>
      <span id="preview-name"></span>
      <span style="display:flex;align-items:center;gap:8px">
        <button class="preview-nav" id="preview-next" aria-label="下一张">›</button>
        <button id="preview-close">完成</button>
      </span>
    </div>
    <div id="preview-body"></div>
  </div>
  <div class="toast" id="toast"></div>
<script>
  var TOKEN = new URLSearchParams(location.search).get('t') || new URLSearchParams(location.search).get('token') || '';
  var curDir = '';
  function setDir(dir) { curDir = dir; refresh(); }
  var toastEl = document.getElementById('toast');
  var toastTimer = null;
  document.getElementById('help-toggle').onclick = function () {
    var box = document.getElementById('help-box');
    var showing = box.style.display === 'none';
    box.style.display = showing ? 'block' : 'none';
  };

  var previewableExt = /\.(jpe?g|png|gif|webp|bmp|mp4|m4v|mov|webm|pdf)$/i;
  var previewItems = [];
  var previewIndex = 0;
  function openPreviewAt(index) {
    if (index < 0 || index >= previewItems.length) return;
    previewIndex = index;
    var item = previewItems[index];
    var url = '/transfer/file?t=' + encodeURIComponent(TOKEN) + '&name=' + encodeURIComponent(item.rootRel) + '&mode=view';
    var body = document.getElementById('preview-body');
    var lower = item.rootRel.toLowerCase();
    if (/\.(mp4|m4v|mov|webm)$/.test(lower)) {
      body.innerHTML = '<video controls autoplay playsinline style="max-width:100%;max-height:100%" src="' + url + '"></video>';
    } else if (/\.pdf$/.test(lower)) {
      body.innerHTML = '<iframe style="width:100%;height:100%;border:0" src="' + url + '"></iframe>';
    } else {
      body.innerHTML = '<img style="max-width:100%;max-height:100%;object-fit:contain" src="' + url + '">';
    }
    document.getElementById('preview-name').textContent = item.rootRel.split('/').pop() + '（' + (index + 1) + '/' + previewItems.length + '）';
    document.getElementById('preview-overlay').style.display = 'flex';
  }
  function closePreview() {
    document.getElementById('preview-body').innerHTML = '';
    document.getElementById('preview-overlay').style.display = 'none';
  }
  document.getElementById('list').addEventListener('click', function (e) {
    if (e.target.closest('.swipe-del') || e.target.closest('.file-del') || e.target.closest('.file-get')) return;
    var row = e.target.closest('.row[data-preview="1"]');
    if (!row) return;
    var name = decodeURIComponent(row.getAttribute('data-name'));
    var index = previewItems.findIndex(function (item) { return item.rootRel === name; });
    if (index === -1) return;
    openPreviewAt(index);
  });
  document.getElementById('preview-close').onclick = closePreview;
  document.getElementById('preview-prev').onclick = function () { var next = neighborPreview(previewIndex, -1); if (next !== -1) openPreviewAt(next); };
  document.getElementById('preview-next').onclick = function () { var next = neighborPreview(previewIndex, 1); if (next !== -1) openPreviewAt(next); };
  var previewTouch = { startX: 0, y: 0 };
  bindPreviewGestures(document.getElementById('preview-overlay'));
  function neighborPreview(from, direction) {
    var step = direction < 0 ? -1 : 1;
    var idx = from + step;
    while (idx >= 0 && idx < previewItems.length) {
      if (!/\.pdf$/i.test(previewItems[idx].rootRel)) return idx;
      idx += step;
    }
    return -1;
  }
  function bindPreviewGestures(el) {
    el.addEventListener('touchstart', function (e) {
      previewTouch.startX = e.touches[0].clientX;
      previewTouch.y = e.touches[0].clientY;
    }, { passive: true });
    el.addEventListener('touchend', function (e) {
      var dx = e.changedTouches[0].clientX - previewTouch.startX;
      var dy = e.changedTouches[0].clientY - previewTouch.y;
      if (Math.abs(dx) < 50 || Math.abs(dx) < Math.abs(dy)) return;
      var next = neighborPreview(previewIndex, dx < 0 ? 1 : -1);
      if (next !== -1) openPreviewAt(next);
    }, { passive: true });
  }


  // ── 左滑露出删除（iOS 原生同款手势，长按也可）──
  var swipeState = { row: null, startX: 0, moved: false, timer: null };
  function closeSwipe() {
    if (swipeState.row) {
      var el = swipeState.row.querySelector('.row-swipe');
      if (el) el.style.transform = '';
    }
    swipeState.row = null;
  }
  function openSwipe(row) {
    closeSwipe();
    var el = row.querySelector('.row-swipe');
    el.style.transform = 'translateX(-78px)';
    swipeState.row = row;
  }
  document.getElementById('list').addEventListener('touchstart', function (e) {
    var row = e.target.closest('.row');
    if (!row || e.touches.length !== 1) return;
    swipeState.row = row;
    swipeState.startX = e.touches[0].clientX;
    swipeState.moved = false;
    var swipe = row.querySelector('.row-swipe');
    swipe.style.transition = 'none';
    row.closest('.card').classList.add('swiping');
  }, { passive: true });
  document.getElementById('list').addEventListener('touchmove', function (e) {
    if (!swipeState.row || e.touches.length !== 1) return;
    var dx = e.touches[0].clientX - swipeState.startX;
    if (Math.abs(dx) > 6) { swipeState.moved = true; clearTimeout(swipeState.timer); }
    if (!swipeState.moved) return;
    var swipe = swipeState.row.querySelector('.row-swipe');
    swipe.style.transition = 'none';
    var base = swipe.style.transform.indexOf('-78') !== -1 ? -72 : 0;
    var offset = Math.max(-78, Math.min(0, base + dx));
    swipe.style.transform = 'translateX(' + offset + 'px)';
    swipeState.dx = offset;
  }, { passive: true });
  document.getElementById('list').addEventListener('touchend', function () {
    clearTimeout(swipeState.timer);
    var card = document.querySelector('.card.swiping');
    if (card) card.classList.remove('swiping');
    if (!swipeState.row) return;
    if (swipeState.axis !== 'x') { swipeState.row = null; return; }
    var swipe = swipeState.row.querySelector('.row-swipe');
    swipe.style.transition = 'transform .18s ease-out';
    if (swipeState.dx < -39) { swipe.style.transform = 'translateX(-78px)'; }
    else { swipe.style.transform = ''; swipeState.row = null; }
  });
  document.getElementById('list').addEventListener('click', function (e) {
    var del = e.target.closest('.swipe-del');
    if (del) {
      var name = decodeURIComponent(del.getAttribute('data-name'));
      fetch('/transfer/delete?t=' + encodeURIComponent(TOKEN) + '&name=' + encodeURIComponent(name), { method: 'POST' })
        .then(function (r) { if (r.ok) { toast('已删除'); refresh(); } else toast('删除失败'); })
        .catch(function () { toast('删除失败，检查网络'); });
      return;
    }
    var row = e.target.closest('.row');
    if (row && row.querySelector('.row-swipe') && row.querySelector('.row-swipe').style.transform.indexOf('-78') !== -1) {
      closeSwipe();
      return;
    }
  });

  function toast(text) {
    toastEl.textContent = text; toastEl.classList.add('show');
    clearTimeout(toastTimer);
    toastTimer = setTimeout(function () { toastEl.classList.remove('show'); }, 2200);
  }
  function fmtSize(n) {
    if (n >= 1073741824) return (n / 1073741824).toFixed(1) + ' GB';
    if (n >= 1048576) return (n / 1048576).toFixed(1) + ' MB';
    if (n >= 1024) return (n / 1024).toFixed(0) + ' KB';
    return n + ' B';
  }
  function fmtTime(ts) {
    if (!ts) return '';
    var d = new Date(ts * 1000);
    function p(x) { return x < 10 ? '0' + x : x; }
    return (d.getMonth() + 1) + '月' + d.getDate() + '日 ' + p(d.getHours()) + ':' + p(d.getMinutes());
  }
  function esc(s) {
    return s.replace(/[&<>"']/g, function (c) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c];
    });
  }
  function refresh() {
    fetch('/transfer/list?t=' + encodeURIComponent(TOKEN) + '&dir=' + encodeURIComponent(curDir)).then(function (r) { return r.json(); }).then(function (data) {
      var box = document.getElementById('list');
      var html = '';
      if (curDir) {
        var parent = curDir.split('/');
        parent.pop();
        html += '<div class="folder-row" data-dir="' + esc(parent.join('/')) + '">📁 返回上级</div>';
      }
      var folderSvg = '<img src="data:image/png;base64,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" style="width:100%;height:100%;object-fit:cover" alt="">';
      html += (data.folders || []).map(function (d) {
        return '<div class="folder-row" data-dir="' + esc(curDir ? curDir + '/' + d.name : d.name) + '">'
          + '<div class="file-thumb">' + folderSvg + '</div>'
          + '<div class="file-name"><span class="file-n">' + esc(d.name) + '</span>'
          + '<div class="file-meta">' + fmtSize(d.size) + ' · ' + fmtTime(d.mtime) + '</div></div>'
          + '</div>';
      }).join('');
      if (!data.ok) {
        box.innerHTML = '<div class="empty">刷新失败</div>';
        return;
      }
      var previewQueue = [];
      box.innerHTML = html + (data.files.length ? data.files.map(function (f) {
        var dot = f.name.lastIndexOf('.');
        var baseName = dot > 0 ? f.name.slice(0, dot) : f.name;
        var ext = (dot > 0 ? f.name.slice(dot + 1) : 'file').toUpperCase();
        var previewable = /\.(jpe?g|png|gif|webp|bmp|mp4|m4v|mov|webm|pdf)$/i.test(f.name);
        var rootRel = curDir ? curDir + '/' + f.name : f.name;
        if (previewable) previewQueue.push({ rootRel: rootRel });
        var fileUrl = '/transfer/file?t=' + encodeURIComponent(TOKEN) + '&name=' + encodeURIComponent(rootRel) + '&mode=view';
        var thumb;
        if (/\.(jpe?g|png|gif|webp|bmp)$/i.test(f.name)) {
          thumb = '<img loading="lazy" src="' + fileUrl + '" alt="">';
        } else if (/\.(mp4|m4v|mov|webm)$/i.test(f.name)) {
          thumb = '<span class="thumb-icon">▶</span>';
        } else {
          thumb = '<span class="thumb-icon thumb-doc">' + esc(ext.slice(0, 4)) + '</span>';
        }
        var rootRel = curDir ? curDir + '/' + f.name : f.name;
        return '<div class="row" data-name="' + encodeURIComponent(rootRel) + '" data-preview="' + (previewable ? '1' : '0') + '">'
          + '<button class="swipe-del" data-name="' + encodeURIComponent(rootRel) + '">删除</button>'
          + '<div class="row-swipe">'
          + '<div class="file-thumb">' + thumb + '</div>'
          + '<div class="file-name"><span class="file-n">' + esc(baseName) + '</span><span class="file-ext">' + esc(ext) + '</span>'
          + '<div class="file-meta">' + fmtSize(f.size) + ' · ' + fmtTime(f.mtime) + '</div></div>'
          + '<div class="file-ops">'
          + '<a class="file-get" href="/transfer/file?t=' + encodeURIComponent(TOKEN) + '&name=' + encodeURIComponent(rootRel) + '">下载</a>'
          + '</div>'
          + '</div>'
          + '</div>';
      }).join('') : '<div class="empty">还没有文件</div>');
      previewItems = previewQueue;
    }).catch(function () { toast('刷新失败'); });
  }
  function upload(input) {
    if (!input.files.length) return;
    var form = new FormData();
    for (var i = 0; i < input.files.length; i++) form.append('file', input.files[i], input.files[i].name);
    var bar = document.getElementById('progress-wrap');
    var fill = document.getElementById('progress-fill');
    var label = document.getElementById('progress-text');
    bar.style.display = 'block';
    fill.style.width = '0%';
    var xhr = new XMLHttpRequest();
    xhr.open('POST', '/transfer/upload?t=' + encodeURIComponent(TOKEN) + '&dir=' + encodeURIComponent(curDir));
    xhr.upload.onprogress = function (e) {
      if (e.lengthComputable) {
        var pct = Math.floor(e.loaded / e.total * 100);
        fill.style.width = pct + '%';
        label.textContent = '上传中 ' + pct + '%（' + fmtSize(e.loaded) + ' / ' + fmtSize(e.total) + '）';
      }
    };
    xhr.onload = function () {
      bar.style.display = 'none';
      try {
        var data = JSON.parse(xhr.responseText);
        if (data.ok) { toast('已传 ' + data.saved.length + ' 个文件'); refresh(); }
        else toast('上传失败：' + (data.status || '未知错误'));
      } catch (err) { toast('上传失败，检查网络'); }
    };
    xhr.onerror = function () { bar.style.display = 'none'; toast('上传失败，检查网络'); };
    xhr.send(form);
    input.value = '';
  }
  document.getElementById('copyurl').onclick = function () {
    var url = location.origin + '/transfer/upload?t=' + encodeURIComponent(TOKEN);
    // https（安全上下文）→ 直接写剪贴板；http 局域网 → 展示并全选，引导长按拷贝
    if (navigator.clipboard && window.isSecureContext) {
      navigator.clipboard.writeText(url)
        .then(function () { toast('上传地址已复制'); })
        .catch(function () { showCopyPrompt(url); });
    } else {
      showCopyPrompt(url);
    }
  };
  function showCopyPrompt(url) {
    var old = document.getElementById('copy-prompt');
    if (old) old.remove();
    var box = document.createElement('div');
    box.id = 'copy-prompt';
    box.style.cssText = 'position:fixed;inset:0;background:rgba(0,0,0,.7);z-index:120;display:flex;align-items:center;justify-content:center';
    box.innerHTML = '<div style="background:#fff;border-radius:14px;padding:16px;max-width:86vw;width:340px">'
      + '<p style="margin:0 0 8px;font-size:14px;font-weight:600;color:#1d1d1f">上传地址（已全选）</p>'
      + '<input readonly id="copy-prompt-input" style="width:100%;box-sizing:border-box;font-size:13px;padding:10px;border-radius:8px;border:1px solid #ddd;color:#1d1d1f">'
      + '<p style="margin:8px 0 0;font-size:12px;color:#6e6e73">iOS 请长按输入框 → 拷贝</p>'
      + '<button id="copy-prompt-close" style="margin-top:12px;width:100%;border:0;border-radius:10px;padding:10px 0;background:#3478f6;color:#fff;font-size:14px;font-weight:600">关闭</button>'
      + '</div>';
    document.body.appendChild(box);
    var input = box.querySelector('input');
    input.value = url;
    input.focus(); input.select(); input.setSelectionRange(0, url.length);
    box.querySelector('#copy-prompt-close').onclick = function () { box.remove(); };
    box.addEventListener('click', function (e) { if (e.target === box) box.remove(); });
  }
  document.getElementById('list').addEventListener('click', function (e) {
    var folderRow = e.target.closest('.folder-row');
    if (folderRow) { setDir(decodeURIComponent(folderRow.getAttribute('data-dir'))); }
  });
  document.getElementById('fab').onclick = function () { document.getElementById('files').click(); };
  document.getElementById('files').onchange = function () { upload(this); };
  refresh();
</script>
</body>
</html>
"#;
