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
    let mut folders: Vec<String> = Vec::new();
    let mut entries: Vec<FileEntry> = Vec::new();
    if let Ok(mut read) = tokio::fs::read_dir(&browse_dir).await {
        while let Ok(Some(entry)) = read.next_entry().await {
            let Ok(meta) = entry.metadata().await else { continue };
            let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
            if meta.is_dir() {
                folders.push(name);
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
    folders.sort();
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
    while let Ok(Some(mut field)) = multipart.next_field().await {
        let Some(raw_name) = field.file_name().map(str::to_string) else {
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
    display: flex; align-items: center; gap: 8px;
    padding: 9px 0; font-size: 13px; font-weight: 600; color: #3478f6;
    cursor: pointer; border-bottom: 1px solid rgba(0,0,0,.06);
  }
  .folder-row:active { opacity: .6; }
  .row { position: relative; overflow: hidden; }
  .row-swipe {
    position: relative; z-index: 1; display: flex; align-items: center; gap: 12px;
    flex: 1; min-width: 0; background: #ffffff; padding: 13px 0;
    transition: transform .2s ease;
  }
  .swipe-del {
    position: absolute; right: 0; top: 0; bottom: 0; width: 72px;
    border: 0; background: #c0392b; color: #fff;
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
  .file-ops { display: flex; flex-direction: column; gap: 6px; flex-shrink: 0; align-items: stretch; }
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
      <p style="font-size:13px;line-height:1.7;margin:0 0 10px">在快捷指令 App 新建指令：<br>① 添加「从共享表单接收」，类型选「图像」或「文件」；<br>② 添加「获取 URL 内容」：方法选 POST，URL 填下面这条，请求体选「表单」，添加字段 file，值选「快捷指令输入」；<br>③ 命名保存。以后在照片里点「分享」→ 选它，照片直接传到这台电脑。</p>
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
    <div id="preview-topbar"><span id="preview-name"></span><button id="preview-close">完成</button></div>
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
    box.style.display = box.style.display === 'none' ? 'block' : 'none';
  };

  var previewableExt = /\.(jpe?g|png|gif|webp|bmp|mp4|m4v|mov|webm)$/i;
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
  var previewTouch = { startX: 0, y: 0 };
  function bindPreviewGestures(el) {
    el.addEventListener('touchstart', function (e) {
      previewTouch.startX = e.touches[0].clientX;
      previewTouch.y = e.touches[0].clientY;
    }, { passive: true });
    el.addEventListener('touchend', function (e) {
      var dx = e.changedTouches[0].clientX - previewTouch.startX;
      var dy = e.changedTouches[0].clientY - previewTouch.y;
      if (Math.abs(dx) < 50 || Math.abs(dx) < Math.abs(dy)) return;
      if (dx < 0) openPreviewAt(previewIndex + 1);
      else openPreviewAt(previewIndex - 1);
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
    el.style.transform = 'translateX(-72px)';
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
    swipeState.timer = setTimeout(function () {
      if (swipeState.row === row && !swipeState.moved) openSwipe(row);
    }, 600);
  }, { passive: true });
  document.getElementById('list').addEventListener('touchmove', function (e) {
    if (!swipeState.row || e.touches.length !== 1) return;
    var dx = e.touches[0].clientX - swipeState.startX;
    if (Math.abs(dx) > 6) { swipeState.moved = true; clearTimeout(swipeState.timer); }
    if (!swipeState.moved) return;
    var swipe = swipeState.row.querySelector('.row-swipe');
    swipe.style.transition = 'none';
    var base = swipe.style.transform.indexOf('-72') !== -1 ? -72 : 0;
    var offset = Math.max(-72, Math.min(0, base + dx));
    swipe.style.transform = 'translateX(' + offset + 'px)';
    swipeState.dx = offset;
  }, { passive: true });
  document.getElementById('list').addEventListener('touchend', function () {
    clearTimeout(swipeState.timer);
    if (!swipeState.row) return;
    var swipe = swipeState.row.querySelector('.row-swipe');
    swipe.style.transition = 'transform .2s ease';
    var match = swipe.style.transform.match(/-?\d+/);
    var offset = match ? parseInt(match[0], 10) : 0;
    if (offset < -36) { swipe.style.transform = 'translateX(-72px)'; swipeState.row = swipeState.row; }
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
    if (row && row.querySelector('.row-swipe') && row.querySelector('.row-swipe').style.transform.indexOf('-72') !== -1) {
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
      html += (data.folders || []).map(function (d) {
        return '<div class="folder-row" data-dir="' + esc(curDir ? curDir + '/' + d : d) + '">📁 ' + esc(d) + '</div>';
      }).join('');
      if (!data.ok || !data.files.length) {
        box.innerHTML = html + '<div class="empty">还没有文件</div>';
        return;
      }
      var previewQueue = [];
      box.innerHTML = data.files.map(function (f) {
        var dot = f.name.lastIndexOf('.');
        var baseName = dot > 0 ? f.name.slice(0, dot) : f.name;
        var ext = (dot > 0 ? f.name.slice(dot + 1) : 'file').toUpperCase();
        var previewable = /\.(jpe?g|png|gif|webp|bmp|mp4|m4v|mov|webm)$/i.test(f.name);
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
      }).join('');
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
    var ta = document.createElement('textarea');
    ta.value = url; ta.style.position = 'fixed'; ta.style.opacity = '0';
    document.body.appendChild(ta); ta.focus(); ta.select();
    try { document.execCommand('copy'); toast('上传地址已复制'); }
    catch (e) { toast('复制失败，请长按手动复制'); }
    ta.remove();
  };
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
