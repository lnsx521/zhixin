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
    let mut entries: Vec<FileEntry> = Vec::new();
    if let Ok(mut read) = tokio::fs::read_dir(&dir).await {
        while let Ok(Some(entry)) = read.next_entry().await {
            let meta = match entry.metadata().await {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !meta.is_file() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
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
    entries.sort_by_key(|a| std::cmp::Reverse(a.mtime));
    entries.truncate(LIST_LIMIT);
    Json(serde_json::json!({ "ok": true, "folder": "transfer", "files": entries })).into_response()
}

/// 图片/视频扩展名判断（按日期归档用）。
fn is_media(name: &str) -> bool {
    let lower = name.to_lowercase();
    [".jpg", ".jpeg", ".png", ".gif", ".webp", ".heic", ".bmp", ".mp4", ".mov"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// 本地日期（YYYY-MM-DD），用于媒体归档子目录（东八区，主场景中国用户）。
fn today_folder() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        + 8 * 3600;
    civil_from_days(secs / 86400)
}

fn civil_from_days(days: i64) -> String {
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
    format!("{y:04}-{m:02}-{d:02}")
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
    let dir = folder(&app);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("文件互传：创建接收文件夹失败：{e}");
        return reply(StatusCode::INTERNAL_SERVER_ERROR, "cannot_create_folder");
    }
    let today = today_folder();
    let mut saved: Vec<String> = Vec::new();
    while let Ok(Some(mut field)) = multipart.next_field().await {
        let Some(raw_name) = field.file_name().map(str::to_string) else {
            continue;
        };
        let Some(name) = sanitize_name(&raw_name) else {
            continue;
        };
        // 图片/视频按日期归档：接收文件夹/图片/2026-11-18/文件名
        let target_dir = if is_media(&name) {
            let sub = dir.join("图片").join(&today);
            let _ = std::fs::create_dir_all(&sub);
            sub
        } else {
            dir.clone()
        };
        let target = unique_path(&target_dir, &name);
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

async fn download(
    State(app): State<AppHandle>,
    AxumPath(name): AxumPath<String>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if let Err(response) = guard(&app, &headers, query_token(&params).as_deref()) {
        return response;
    }
    let Some(name) = sanitize_name(&name) else {
        return reply(StatusCode::BAD_REQUEST, "invalid_name");
    };
    let path = folder(&app).join(&name);
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => return reply(StatusCode::NOT_FOUND, "not_found"),
    };
    let length = file.metadata().await.map(|m| m.len()).unwrap_or(0);
    let encoded = utf8_percent_encode(&name, NON_ALPHANUMERIC_KEPT);
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename*=UTF-8''{encoded}"),
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
        .route("/transfer/file/{name}", get(download))
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
  @media (prefers-color-scheme: dark) {
    body { background: #1e1e20; color: #f2f2f4; }
    .card { background: #2a2a2d; border-color: #3a3a3e; }
    .muted { color: #a6a6ab; }
    .file-meta { color: #a6a6ab; }
    .empty { color: #a6a6ab; }
    .row { border-bottom-color: rgba(255,255,255,.09); }
    .btn.secondary { background: #2c3d5c; color: #8ab4ff; }
    .file-get { background: #2c3d5c; color: #8ab4ff; }
    .toast { background: rgba(60,60,64,.96); }
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
  .row { display: flex; align-items: center; gap: 12px; padding: 13px 0; border-bottom: 1px solid rgba(0,0,0,.06); }
  .row:last-child { border-bottom: 0; }
  .file-name { flex: 1; min-width: 0; overflow-wrap: anywhere; font-size: 14.5px; }
  .file-meta { color: #6e6e73; font-size: 11.5px; margin-top: 2px; }
  .file-get {
    flex-shrink: 0; border: 0; border-radius: 9px; padding: 8px 14px;
    background: #e9f1fe; color: #3478f6; font-size: 13px; font-weight: 600;
    text-decoration: none; display: inline-block;
  }
  .empty { text-align: center; color: #6e6e73; padding: 18px 0 6px; font-size: 13.5px; }
  .toast {
    position: fixed; left: 50%; bottom: 28px; transform: translateX(-50%) translateY(12px);
    background: rgba(29,29,31,.92); color: #fff; border-radius: 999px; padding: 9px 20px;
    font-size: 13px; opacity: 0; transition: all .25s; pointer-events: none; max-width: 86vw;
  }
  .toast.show { opacity: 1; transform: translateX(-50%) translateY(0); }
</style>
</head>
<body>
  <div class="card">
    <h1>知信 · 文件互传</h1>
    <p class="muted">上传的文件会出现在电脑的接收文件夹；下方列表可下载到手机。</p>
  </div>
  <div class="card">
    <button class="btn" id="pick">选择文件上传</button>
    <button class="btn secondary" id="camera">拍照或录像上传</button>
    <input type="file" id="files" multiple>
    <input type="file" id="capture" accept="image/*,video/*" capture="environment">
  </div>
  <div class="card">
    <p class="muted" style="margin:0 0 6px">配到 iPhone「分享」菜单（可选）</p>
    <p style="font-size:13.5px;line-height:1.75;margin:0 0 12px">在快捷指令 App 新建指令：<br>① 添加「从共享表单接收」，类型选「图像」或「文件」；<br>② 添加「获取 URL 内容」：方法选 POST，URL 填下面这条，请求体选「表单」，添加字段 file，值选「快捷指令输入」；<br>③ 命名保存。以后在照片里点「分享」→ 选它，照片直接传到这台电脑。</p>
    <button class="btn secondary" id="copyurl">复制上传地址</button>
  </div>
  <div class="card">
    <p class="muted" style="margin:0 0 4px">电脑上的文件</p>
    <div id="list"></div>
  </div>
  </div>
  <div class="card" id="progress-wrap" style="display:none">
    <div style="height:8px;border-radius:6px;background:#e2e0d8;overflow:hidden">
      <div id="progress-fill" style="height:100%;width:0%;background:#3b82d6;border-radius:6px;transition:width .2s"></div>
    </div>
    <p id="progress-text" style="font-size:12px;color:#5f5e5a;margin:6px 0 0">上传中…</p>
  </div>
  <div class="toast" id="toast"></div>
<script>
  var TOKEN = new URLSearchParams(location.search).get('t') || new URLSearchParams(location.search).get('token') || '';
  var toastEl = document.getElementById('toast');
  var toastTimer = null;
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
    fetch('/transfer/list?t=' + encodeURIComponent(TOKEN)).then(function (r) { return r.json(); }).then(function (data) {
      var box = document.getElementById('list');
      if (!data.ok || !data.files.length) {
        box.innerHTML = '<div class="empty">还没有文件</div>';
        return;
      }
      box.innerHTML = data.files.map(function (f) {
        return '<div class="row"><div class="file-name">' + esc(f.name)
          + '<div class="file-meta">' + fmtSize(f.size) + ' · ' + fmtTime(f.mtime) + '</div></div>'
          + '<a class="file-get" href="/transfer/file/' + encodeURIComponent(f.name) + '?t=' + encodeURIComponent(TOKEN) + '">下载</a></div>';
      }).join('');
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
    xhr.open('POST', '/transfer/upload?t=' + encodeURIComponent(TOKEN));
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
  document.getElementById('pick').onclick = function () { document.getElementById('files').click(); };
  document.getElementById('camera').onclick = function () { document.getElementById('capture').click(); };
  document.getElementById('files').onchange = function () { upload(this); };
  document.getElementById('capture').onchange = function () { upload(this); };
  refresh();
</script>
</body>
</html>
"#;
