//! App 图标仓库：联网获取（苹果 iTunes 接口）+ 本地缓存 + 手动替换。
//!
//! 优先级：用户手动设置的 > 联网缓存的 > 内置品牌图标（前端 BRAND_ASSETS）。
//! 缓存目录：应用数据目录 /icons/{bundle_id}.png；索引用 sidecar JSON 记录官方 App 名与手动标记。
//! 绝不把通知的 origin（好友备注/群名/手机号）写入索引——那是隐私，不是 App 名。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::paths;

fn cache_dir(app: &AppHandle) -> PathBuf {
    paths::data_dir(app).join("icons")
}

fn index_path(app: &AppHandle) -> PathBuf {
    cache_dir(app).join("index.json")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct IconIndex {
    /// bundle id → 名称与是否手动替换。
    entries: BTreeMap<String, IconEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct IconEntry {
    /// 旧索引可能存过 origin（好友备注/群名/手机号）。只在读取时兼容，绝不写回磁盘。
    #[serde(default, skip_serializing)]
    name: String,
    /// 苹果接口返回的官方 App 名（非会话标题，不含隐私）。
    #[serde(default)]
    app_name: String,
    #[serde(default)]
    manual: bool,
}

fn load_index(app: &AppHandle) -> IconIndex {
    let path = index_path(app);
    let mut index: IconIndex = std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    // 迁移：清掉旧索引里写入的会话标题，并回写覆盖磁盘原文。
    if index.entries.values().any(|entry| !entry.name.is_empty()) {
        for entry in index.entries.values_mut() {
            entry.name.clear();
        }
        let _ = save_index(app, &index);
    }
    index
}

fn save_index(app: &AppHandle, index: &IconIndex) -> Result<(), String> {
    let dir = cache_dir(app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(index).map_err(|e| e.to_string())?;
    std::fs::write(index_path(app), bytes).map_err(|e| e.to_string())
}

fn cached_path(app: &AppHandle, app_id: &str) -> Option<PathBuf> {
    let safe: String = app_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let path = cache_dir(app).join(format!("{safe}.png"));
    path.exists().then_some(path)
}

fn to_data_url(bytes: &[u8]) -> String {
    use base64::Engine as _;
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// 图标数据 URL：缓存/手动文件优先（手动标记的也在这），没有则 None（前端用内置品牌图或兜底）。
#[tauri::command]
pub fn get_app_icon(
    app: AppHandle,
    window: tauri::WebviewWindow,
    app_id: String,
) -> Option<String> {
    if window.label() != "main" && !window.label().starts_with("toast") && !window.label().starts_with("expanded") {
        return None;
    }
    let path = cached_path(&app, &app_id)?;
    let bytes = std::fs::read(path).ok()?;
    Some(to_data_url(&bytes))
}

/// 缓存清单（图标设置 UI 用）。
#[derive(Debug, Clone, Serialize)]
pub struct IconCacheEntry {
    pub app_id: String,
    pub app_name: String,
    pub manual: bool,
}

#[tauri::command]
pub fn list_icon_cache(app: AppHandle) -> Vec<IconCacheEntry> {
    // 旧缓存只有图标没名字：后台补一次（只查名字，不重下图标），前端稍后回刷。
    spawn_fill_names(app.clone());
    let index = load_index(&app);
    index
        .entries
        .iter()
        .map(|(id, entry)| IconCacheEntry {
            app_id: id.clone(),
            app_name: entry.app_name.clone(),
            manual: entry.manual,
        })
        .collect()
}

/// 手动替换：前端传来 base64 PNG 数据 URL，写入缓存并标记 manual。
#[tauri::command]
pub fn set_app_icon(
    app: AppHandle,
    app_id: String,
    data_url: String,
) -> Result<(), String> {
    let bytes = decode_data_url(&data_url)?;
    let dir = cache_dir(&app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe: String = app_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    std::fs::write(dir.join(format!("{safe}.png")), bytes).map_err(|e| e.to_string())?;

    let mut index = load_index(&app);
    let entry = index.entries.entry(app_id.clone()).or_default();
    entry.manual = true;
    save_index(&app, &index)
}

/// 恢复官方图标：删除缓存文件与手动标记（下次联网自动重新下载）。
#[tauri::command]
pub fn reset_app_icon(app: AppHandle, app_id: String) -> Result<(), String> {
    if let Some(path) = cached_path(&app, &app_id) {
        let _ = std::fs::remove_file(path);
    }
    let mut index = load_index(&app);
    index.entries.remove(&app_id);
    save_index(&app, &index)
}

/// 清空全部联网下载的图标（手动设置的保留）。
#[tauri::command]
pub fn clear_icon_cache(app: AppHandle) -> Result<(), String> {
    let index = load_index(&app);
    let dir = cache_dir(&app);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let id = entry.file_name().to_string_lossy().trim_end_matches(".png").to_string();
            let manual = index.entries.get(&id).map(|e| e.manual).unwrap_or(false);
            if !manual {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}

fn decode_data_url(data_url: &str) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    let payload = data_url
        .strip_prefix("data:image/png;base64,")
        .ok_or("仅支持 PNG data URL")?;
    base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|e| e.to_string())
}

const LOOKUP_URL: &str = "https://itunes.apple.com/lookup?country=cn&entity=software&bundleId=";

/// 后台联网获取图标：按 bundle id 查苹果官方接口 → 下载 512px 图标 → 写缓存。
/// 失败静默（下次通知再试）；成功后下一条该 App 的通知就用新图标。
pub fn spawn_fetch(app: AppHandle, app_id: String) {
    if app_id.trim().is_empty() {
        return;
    }
    if cached_path(&app, &app_id).is_some() {
        let index = load_index(&app);
        let known = index.entries.get(&app_id);
        // 手动图标绝不动；已有官方名的缓存也不用再联网。
        // 旧缓存只有图标没名字时，补一次请求把名字取回来。
        if known.map(|e| e.manual || !e.app_name.is_empty()).unwrap_or(false) {
            return;
        }
    }
    {
        use zhixin_core::config::Config;
        let fetch_enabled = Config::load_or_default(paths::config_path(&app))
            .map(|config| config.general.icon_fetch)
            .unwrap_or(false);
        if !fetch_enabled {
            return;
        }
    }
    tauri::async_runtime::spawn(async move {
        if let Err(error) = fetch_icon(app, app_id.clone()).await {
            log::warn!("图标获取失败（{app_id}）：{error}");
        }
    });
}

async fn fetch_icon(app: AppHandle, app_id: String) -> Result<(), String> {
    {
            let lookup = format!("https://itunes.apple.com/lookup?country=cn&entity=software&bundleId={}", urlencoding_encode(&app_id));
            let response = reqwest::get(&lookup).await.map_err(|e| e.to_string())?;
            let parsed: iTunesResponse = response.json().await.map_err(|e| e.to_string())?;
            let result = parsed.results.into_iter().next().ok_or("苹果接口未找到该 App")?;
            let track_name = result.track_name.unwrap_or_default();
            let artwork = result
                .artwork_url_512
                .ok_or("苹果接口未找到该 App 的图标")?;
            let bytes = reqwest::get(&artwork)
                .await
                .map_err(|e| e.to_string())?
                .bytes()
                .await
                .map_err(|e| e.to_string())?
                .to_vec();
            let dir = cache_dir(&app);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let safe: String = app_id
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
                .collect();
            std::fs::write(dir.join(format!("{safe}.png")), bytes).map_err(|e| e.to_string())?;
            let mut index = load_index(&app);
            let entry = index.entries.entry(app_id.clone()).or_default();
            entry.app_name = track_name;
            save_index(&app, &index)
    }
}

fn urlencoding_encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct iTunesResponse {
    results: Vec<iTunesResult>,
}

#[derive(Debug, Deserialize)]
struct iTunesResult {
    #[serde(rename = "artworkUrl512", default)]
    artwork_url_512: Option<String>,
    #[serde(rename = "trackName", default)]
    track_name: Option<String>,
}

/// 打开图标设置页时后台补齐旧缓存缺失的官方名：只查 iTunes，不重新下载图标。
pub fn spawn_fill_names(app: AppHandle) {
    use zhixin_core::config::Config;
    let enabled = Config::load_or_default(paths::config_path(&app))
        .map(|config| config.general.icon_fetch)
        .unwrap_or(false);
    if !enabled {
        return;
    }
    let targets: Vec<String> = load_index(&app)
        .entries
        .iter()
        .filter(|(_, entry)| entry.app_name.is_empty() && !entry.manual)
        .map(|(id, _)| id.clone())
        .collect();
    for app_id in targets {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let lookup = format!(
                "https://itunes.apple.com/lookup?country=cn&entity=software&bundleId={}",
                urlencoding_encode(&app_id)
            );
            let name = async {
                let response = reqwest::get(&lookup).await.map_err(|e| e.to_string())?;
                let parsed: iTunesResponse = response.json().await.map_err(|e| e.to_string())?;
                Ok::<String, String>(
                    parsed
                        .results
                        .into_iter()
                        .next()
                        .and_then(|result| result.track_name)
                        .unwrap_or_default(),
                )
            }
            .await;
            let Ok(name) = name else { return };
            if name.is_empty() {
                return;
            }
            let mut index = load_index(&app);
            if let Some(entry) = index.entries.get_mut(&app_id) {
                if !entry.app_name.is_empty() {
                    return;
                }
                entry.app_name = name;
                let _ = save_index(&app, &index);
            }
        });
    }
}

/// 打开图标缓存目录（用户可手动替换 PNG）。
#[tauri::command]
pub fn open_icons_dir(app: AppHandle) -> Result<(), String> {
    let dir = cache_dir(&app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::process::Command::new("explorer")
        .arg(dir)
        .spawn()
        .map_err(|error| format!("打开资源管理器失败：{error}"))?;
    Ok(())
}
