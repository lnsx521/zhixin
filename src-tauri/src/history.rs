//! 通知历史中心：所有弹过的通知留档，支持回查。
//!
//! 存储为 JSON 文件（通知内容敏感，仅本地），上限 500 条、先进先出。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use zhixin_core::config::Config;
use zhixin_core::model::PhoneNotification;

use crate::paths;

const CAP: usize = 500;

fn history_path(app: &AppHandle) -> PathBuf {
    paths::data_dir(app).join("notification-history.json")
}

fn load(app: &AppHandle) -> Vec<HistoryEntry> {
    std::fs::read(history_path(app))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, entries: &[HistoryEntry]) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(entries).map_err(|e| e.to_string())?;
    std::fs::write(history_path(app), bytes).map_err(|e| e.to_string())
}

/// 修剪：按保留天数清掉过期记录（新通知到达、主窗口打开历史时都会触发）。
pub fn prune(app: &AppHandle) {
    let days = {
        match Config::load_or_default(paths::config_path(app)) {
            Ok(config) => config.notifications.history_limit,
            Err(_) => 3,
        }
    };
    if days == 0 {
        if let Ok(_) = save(app, &[]) {
            let _ = app.emit("history-updated", ());
        }
        return;
    }
    let mut entries = load(app);
    let before = entries.len();
    let cutoff_ms = rolling_cutoff_ms(days);
    entries.retain(|entry| entry.received_at >= cutoff_ms);
    if entries.len() != before {
        let _ = save(app, &entries);
        let _ = app.emit("history-updated", ());
    }
}

/// 滚动覆盖截止线：从现在往前数 N 个 24 小时（监控录像式，新的覆盖旧的）。
fn rolling_cutoff_ms(days: u32) -> u64 {
    (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64)
        .saturating_sub((days as u64) * 86_400_000)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// 收到时间（Unix 毫秒）。
    pub received_at: u64,
    pub app_id: Option<String>,
    pub origin: String,
    pub body: String,
    pub code: Option<String>,
    /// iOS ANCS 通知分类：1 = 来电；其余场景 None。
    pub category: Option<u8>,
    // ── 以下可选字段仅来电/部分来源携带 ───────────────────────────────
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    /// ANCS Notification UID（拒接动作要用）；历史展开/HTTP 入为 None。
    pub uid: Option<u32>,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub message: Option<String>,
    /// 陌生号码运营商描述（"中国移动 · 手机号"）；已存联系人或短信为 None。
    pub carrier: Option<String>,
    /// 通话结果（仅来电类有效）：None=响铃中/来电通知、Some("answered")=接通、Some("missed")=未接。
    pub call_outcome: Option<String>,
}

/// 弹窗时留档（在 popups::show 里调用；全屏暂存的通知在补弹时也会走到这里）。
/// 保留天数跟随设置（notifications.history_limit = 天数，滚动覆盖；0 = 不保存）。
pub fn record(app: &AppHandle, notification: &PhoneNotification) {
    let (keep, days) = {
        match Config::load_or_default(paths::config_path(app)) {
            Ok(config) => (
                config.notifications.keeps_in_history(notification.app_identifier.as_deref().unwrap_or("")),
                config.notifications.history_limit,
            ),
            Err(_) => (true, 3),
        }
    };
    if !keep || days == 0 {
        // 不保留或天数设为 0：直接清空存量记录，避免旧数据残留在主窗口。
        let _ = save(app, &[]);
        return;
    }
    let mut entries = load(app);

    let origin = notification.origin();
    let received_at_ms = notification
        .received_at
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    // 来电合并：同一次通话的状态滚动更新（来电 → 当前通话 → 已接通/未接），
    // 否则 iOS 会把一轮通话拆成 2~3 条通知，历史里出现好几条重复记录。
    //
    // ★ 判据不能用 uid 或号码（实测都会变）：
    //   - ANCS uid 跨 App、跨天复用（uid=30 先后属于电话和微信）
    //   - 一轮通话里 iOS 会换 uid：来电 uid=52「宽带号-电商」→ 接听 uid=53「通话」
    //   所以改用「同包名 + 10 分钟内 + 上一条尚未终结」：
    //   一轮通话的来电/当前通话都是 call_outcome=None，会被合并；一终结就不认。
    let is_ongoing_call = notification.is_call()
        || notification.category == Some(12)
        || matches!(
            notification.app_identifier.as_deref(),
            Some(id) if id.eq_ignore_ascii_case("com.tencent.xin")
        );
    if is_ongoing_call {
        const CALL_MERGE_WINDOW_MS: u64 = 10 * 60_000;
        let target = entries.iter_mut().rev().find(|entry| {
            let entry_is_call = entry.category == Some(1)
                || entry.category == Some(12)
                || entry
                    .app_id
                    .as_deref()
                    .is_some_and(|id| id.eq_ignore_ascii_case("com.apple.mobilephone"));
            entry.app_id.as_deref() == notification.app_identifier.as_deref()
                && received_at_ms.saturating_sub(entry.received_at) < CALL_MERGE_WINDOW_MS
                && (entry.call_outcome.is_none() || entry.uid == Some(notification.uid))
                && (entry_is_call || entry.call_outcome.is_some())
        });
        if let Some(existing) = target {
            // 保留首次响铃时间与来电人：后续「当前通话」会把 title 改成占位符"通话"，
            // 直接用它覆盖会丢掉真正的号码/联系人名（如"宽带号-电商"）。
            // 正文与通话结果跟随最新事件。
            existing.body = notification.summary();
            existing.call_outcome = notification.call_outcome.clone();
            let placeholder_title = notification
                .title
                .as_deref()
                .is_some_and(|t| t == "通话" || t.trim().is_empty());
            if !placeholder_title || existing.title.as_deref().unwrap_or("").trim().is_empty() {
                existing.title = notification.title.clone();
            }
            // subtitle 同样是 iOS 附加信息：「当前通话」那条常常不给（None），
            // 直接覆盖会把来电阶段的归属地/通讯录标签冲掉。空值则保留原值。
            if notification
                .subtitle
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
            {
                existing.subtitle = notification.subtitle.clone();
            }
            existing.message = notification.message.clone();
            if existing.carrier.is_none() {
                // describe() 内部会过滤空格/分隔符，直接传原号码即可；
                // 不能用 extract_longest_digits（"137 7444 0207" 只会取到 "7444"）。
                existing.carrier = zhixin_core::carrier::describe(&origin);
            }
            let _ = save(app, &entries);
            let _ = app.emit("history-updated", ());
            return;
        }
    }

    entries.push(HistoryEntry {
        received_at: received_at_ms,
        app_id: notification.app_identifier.clone(),
        origin,
        body: notification.summary(),
        code: notification.code.clone(),
        category: notification.category,
        device_id: Some(notification.device_id.clone()),
        device_name: notification.device_name.clone(),
        uid: Some(notification.uid),
        title: notification.title.clone(),
        subtitle: notification.subtitle.clone(),
        message: notification.message.clone(),
        carrier: if notification.is_call() {
            zhixin_core::carrier::describe(&notification.origin())
        } else {
            None
        },
        call_outcome: notification.call_outcome.clone(),
    });
    // 滚动覆盖（监控录像式）：从现在往前数 N 个 24 小时，过期的直接丢弃
    let cutoff_ms = rolling_cutoff_ms(days);
    entries.retain(|entry| entry.received_at >= cutoff_ms);
    let _ = save(app, &entries);
    // 广播历史变更，主窗口监听后实时刷新列表（否则新消息要重开窗口才可见）。
    let _ = app.emit("history-updated", ());
}

/// 某号码的历史来电统计（来电次数 / 首次 / 最近）。
#[derive(Debug, Clone, Serialize)]
pub struct CallStats {
    pub count: u32,
    pub first_at: Option<u64>,
    pub last_at: Option<u64>,
}

/// 按号码找最近一次通话留下的非空副标题（归属地/通讯录标签）。
/// iOS 的「当前通话」「通话结束」通知常不带 subtitle，弹窗靠这里回填。
pub fn recent_call_subtitle(app: &AppHandle, origin: &str) -> Option<String> {
    load(app)
        .into_iter()
        .rev()
        .find(|entry| {
            entry.origin == origin
                && entry
                    .subtitle
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty())
        })
        .and_then(|entry| entry.subtitle)
}

/// 统计同一号码的历史来电（主窗口来电详情用）。
#[tauri::command]
pub fn get_call_stats(app: AppHandle, origin: String) -> CallStats {
    let entries = load(&app);
    let is_call_entry = |entry: &HistoryEntry| {
        entry.category == Some(1)
            || entry
                .app_id
                .as_deref()
                .is_some_and(|id| id.eq_ignore_ascii_case("com.apple.mobilephone"))
    };
    let calls: Vec<&HistoryEntry> = entries
        .iter()
        .filter(|entry| entry.origin == origin && is_call_entry(entry))
        .collect();
    CallStats {
        count: calls.len() as u32,
        first_at: calls.iter().map(|entry| entry.received_at).min(),
        last_at: calls.iter().map(|entry| entry.received_at).max(),
    }
}

#[tauri::command]
pub fn list_notification_history(
    app: AppHandle,
    query: Option<String>,
    limit: Option<u32>,
) -> Vec<HistoryEntry> {
    // 主窗口打开历史时先按保留天数修剪一次：跨天后的过期记录立即消失，不必等下一条消息
    prune(&app);
    let mut entries = load(&app);
    entries.reverse(); // 最新在前
    if let Some(query) = query.as_deref().filter(|q| !q.trim().is_empty()) {
        let needle = query.to_lowercase();
        entries.retain(|entry| {
            entry.origin.to_lowercase().contains(&needle)
                || entry.body.to_lowercase().contains(&needle)
                || entry.code.as_deref().map(|c| c.contains(&needle)).unwrap_or(false)
        });
    }
    entries.truncate(limit.unwrap_or(100) as usize);
    // 存量记录（旧版本写入时 carrier 可能为空）在这里补算一次，不改磁盘文件：
    // 通话类且运营商为空时，从号码实时识别（如 "137 7444 0207" → 中国移动 · 手机号）。
    for entry in &mut entries {
        if entry.carrier.is_none() && is_call_history_entry(entry) {
            entry.carrier = zhixin_core::carrier::describe(&entry.origin);
        }
    }
    entries
}

/// 这条历史记录是否是通话类（跟详情面板 isCallEntry 保持同一判据）。
fn is_call_history_entry(entry: &HistoryEntry) -> bool {
    entry.category == Some(1)
        || entry.category == Some(12)
        || entry
            .app_id
            .as_deref()
            .is_some_and(|id| id.eq_ignore_ascii_case("com.apple.mobilephone"))
}

#[tauri::command]
pub fn clear_notification_history(app: AppHandle) -> Result<(), String> {
    save(&app, &[])?;
    // 广播给主窗口/历史页监听，实时清空左侧列表（否则 JSON 空了但界面还留着旧记录）
    let _ = app.emit("history-updated", ());
    Ok(())
}

/// 导出为文本文件（存到应用数据目录，导出后自动打开所在文件夹）。
#[tauri::command]
pub fn export_notification_history(app: AppHandle) -> Result<String, String> {
    let entries = load(&app);
    let mut lines = Vec::with_capacity(entries.len() + 2);
    lines.push(format!("知信通知历史导出 · {} 条", entries.len()));
    for entry in entries.iter().rev() {
        let time = chrono_fmt(entry.received_at);
        lines.push(format!(
            "[{time}] {} — {}\n{}",
            entry.origin,
            entry.app_id.as_deref().unwrap_or("-"),
            entry.body
        ));
    }
    let text = lines.join("\n\n");
    let path = paths::data_dir(&app).join("通知历史导出.txt");
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    std::process::Command::new("explorer")
        .arg(path.parent().ok_or("无父目录")?)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

fn chrono_fmt(ms: u64) -> String {
    // 简单格式化：本地时区处理依赖前端展示，这里用系统偏移量近似。
    let secs = (ms / 1000) as i64;
    let local = time_offset_secs();
    let total = secs + local;
    let days = total / 86400;
    let rem = total % 86400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let date = days_to_date(days);
    format!("{date} {h:02}:{m:02}:{s:02}")
}

/// 从文本里提取最长连续数字串（号码识别用）。
fn extract_longest_digits(text: &str) -> String {
    let mut best = String::new();
    let mut current = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            current.push(c);
        } else {
            if current.chars().count() > best.chars().count() {
                best = current.clone();
            }
            current.clear();
        }
    }
    if current.chars().count() > best.chars().count() {
        best = current;
    }
    best
}

fn time_offset_secs() -> i64 {
    // 通过对比 UTC 与本地时间戳获取偏移（依赖 std，不引入 chrono）。
    // 方案：用环境变量 TZ 不可靠，改为在 JS 侧格式化时间更准 —— 此处仅作降级展示。
    0
}

fn days_to_date(days: i64) -> String {
    // Unix epoch（1970-01-01）起的天数转 Y-M-D（民用历算法）。
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}
