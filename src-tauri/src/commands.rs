//! 前端可调用的命令。
//!
//! 命名规则：名词_动词。凡是会改状态的都返回 `Result<_, String>`，
//! 错误信息直接给人看。

use log::{info, warn};
use serde::Serialize;
use zhixin_core::ancs::Attributes;
use zhixin_core::config::Config;
use zhixin_core::model::PhoneNotification;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::devices::ManagedDevicePayload;
use crate::state::AppState;
use crate::types::{CaretLayout, LinkStatePayload, ToastPayload};
use crate::{caret, link_worker, paths, popups, tray};

// ── 设置窗口 ──────────────────────────────────────────────────

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Config {
    state.config()
}

#[tauri::command]
pub fn save_config(app: AppHandle, config: Config) -> Result<(), String> {
    for (index, rule) in config.notifications.app_rules.iter().enumerate() {
        if rule.enabled {
            rule.validate()?;
            if config.notifications.app_rules[..index]
                .iter()
                .any(|other| other.enabled && other.app_id.eq_ignore_ascii_case(&rule.app_id))
            {
                return Err("同一来源只能启用一条打开规则".into());
            }
        }
    }
    let path = paths::config_path(&app);

    config
        .save(&path)
        .map_err(|error| format!("保存配置失败：{error}"))?;

    let theme = config.general.theme;
    app.state::<AppState>().apply_config(config);
    caret::config_applied(&app);
    if let Err(error) = app.emit("theme-changed", theme) {
        warn!("广播外观设置失败：{error}");
    }
    info!("配置已保存");

    Ok(())
}

/// 设置界面要的元信息（版本、路径、自启状态）。
#[derive(Debug, Clone, Serialize)]
pub struct SettingsMeta {
    pub version: String,
    pub config_path: String,
    pub log_dir: String,
    pub autostart_enabled: bool,
}

#[tauri::command]
pub fn get_settings_meta(app: AppHandle) -> SettingsMeta {
    use tauri_plugin_autostart::ManagerExt;

    SettingsMeta {
        version: app.package_info().version.to_string(),
        config_path: paths::config_path(&app).display().to_string(),
        log_dir: paths::log_dir(&app).display().to_string(),
        autostart_enabled: app.autolaunch().is_enabled().unwrap_or(false),
    }
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;

    let autostart = app.autolaunch();

    let result = if enabled {
        autostart.enable()
    } else {
        autostart.disable()
    };

    result.map_err(|error| format!("设置开机自启失败：{error}"))?;

    autostart
        .is_enabled()
        .map_err(|error| format!("读取开机自启状态失败：{error}"))
}

#[tauri::command]
pub fn open_config_dir(app: AppHandle) -> Result<(), String> {
    open_in_explorer(&paths::data_dir(&app))
}

#[tauri::command]
pub fn open_logs_dir(app: AppHandle) -> Result<(), String> {
    open_in_explorer(&paths::log_dir(&app))
}

#[tauri::command]
pub fn open_bluetooth_settings() -> Result<(), String> {
    std::process::Command::new("cmd")
        .args(["/c", "start", "ms-settings:bluetooth"])
        .spawn()
        .map_err(|error| format!("打开蓝牙设置失败：{error}"))?;

    Ok(())
}

fn open_in_explorer(path: &std::path::Path) -> Result<(), String> {
    std::process::Command::new("explorer")
        .arg(path)
        .spawn()
        .map_err(|error| format!("打开资源管理器失败：{error}"))?;

    Ok(())
}

/// 仅允许设置窗口打开固定项目链接，不接受前端传入任意 URL 或命令。
#[tauri::command]
pub fn open_project_link(window: tauri::WebviewWindow, target: String) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Project links are only available in settings".into());
    }
    let url = match target.as_str() {
        "repository" => "https://github.com/lnsx521/zhixin",
        "maintainer" => "https://github.com/lnsx521",
        "contributors" => "https://github.com/lnsx521/zhixin/graphs/contributors",
        "issues" => "https://github.com/lnsx521/zhixin/issues",
        "author" => "https://github.com/fxaxg",
        _ => return Err("Unknown project link".into()),
    };
    crate::app_launcher::open(&zhixin_core::app_rules::AppRule {
        app_id: "zhixin.about".into(),
        name: "知信 project".into(),
        enabled: true,
        target: url.into(),
    })
}

// ── 链路 ──────────────────────────────────────────────────────

#[tauri::command]
pub fn get_link_state(app_state: State<'_, AppState>) -> LinkStatePayload {
    let guard = app_state
        .link_state
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (link_state, detail) = guard.clone();
    let verified = link_state.is_ready()
        && app_state
            .link_verified
            .load(std::sync::atomic::Ordering::Relaxed);

    LinkStatePayload {
        state: crate::types::state_key(link_state).to_string(),
        label: link_state.describe().to_string(),
        detail,
        ready: verified,
        awaiting_verification: link_state.is_ready() && !verified,
    }
}

#[tauri::command]
pub fn list_devices(state: State<'_, AppState>) -> Vec<ManagedDevicePayload> {
    state
        .devices
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .payloads()
}

#[tauri::command]
pub fn set_preferred_device(app: AppHandle, id: String) -> Result<(), String> {
    app.state::<AppState>()
        .devices
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .set_preferred(&id)?;
    link_worker::emit_devices(&app);
    Ok(())
}

#[tauri::command]
pub fn set_device_enabled(app: AppHandle, id: String, enabled: bool) -> Result<(), String> {
    app.state::<AppState>()
        .devices
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .set_enabled(&id, enabled)?;
    link_worker::emit_devices(&app);
    Ok(())
}

#[tauri::command]
pub fn forget_device(app: AppHandle, id: String) -> Result<(), String> {
    app.state::<AppState>()
        .devices
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .forget(&id)?;
    link_worker::emit_devices(&app);
    Ok(())
}

/// 造一条测试通知，走和真实通知完全相同的路径（去重除外——每次换新 uid）。
///
/// 点击立即发送，无延迟。
#[tauri::command]
pub fn send_test_notification(app: AppHandle, kind: Option<String>) {
    send_test_kind(app, kind.as_deref().unwrap_or("sms"));
}

/// 托盘菜单也用同一个入口（托盘回调拿不到 `State`，只能拿 `AppHandle`）。
pub fn send_test_notification_from(app: AppHandle) {
    send_test_kind(app, "sms");
}

/// 按 kind 构造不同样式的测试通知（sms/call/wechat/mail/generic）。
pub fn send_test_kind(app: AppHandle, kind: &str) {
    let kind = kind.to_string();
    std::thread::spawn(move || {
        let uid = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.subsec_nanos())
            .unwrap_or(1);

        let kind = kind.as_str();
        let (app_id, title, message, category) = match kind {
            "call" => (
                "com.apple.facetime",
                "13856781234",
                "来电",
                Some(1u8),
            ),
            "wechat" => (
                "com.tencent.xin",
                "李工（设计部）",
                "装配图纸我发你邮箱了，注意查收",
                Some(4u8),
            ),
            "mail" => (
                "com.apple.mail",
                "供应商 A",
                "报价单已更新，请查阅附件",
                Some(6u8),
            ),
            "generic" => (
                "com.some.unknown.app",
                "某应用",
                "您有一条新消息提醒",
                None,
            ),
            // 默认：短信 · 验证码
            _ => (
                "com.apple.MobileSMS",
                "106835812921010",
                "这是一条测试通知，验证码：123456。",
                None,
            ),
        };

        let notification = PhoneNotification::from_ancs(
            "test-device",
            Some("测试".to_string()),
            Attributes {
                uid,
                app_identifier: Some(app_id.to_string()),
                title: Some(title.to_string()),
                message: Some(message.to_string()),
                ..Default::default()
            },
            category,
        );

        info!("发送测试通知（{kind}）");
        link_worker::dispatch_notification(&app, notification);
    });
}

// ── 自定义背景 ────────────────────────────────────────────────

/// 保存用户上传的自定义背景图（原样落盘）。
#[tauri::command]
pub fn save_background_image(app: AppHandle, data: Vec<u8>) -> Result<(), String> {
    let path = crate::paths::background_image_path(&app);
    std::fs::write(&path, &data).map_err(|e| e.to_string())?;
    info!("自定义背景已保存（{} 字节）", data.len());
    Ok(())
}

/// 读取自定义背景图（二进制原样返回；没有则返回空）。
#[tauri::command]
pub fn get_background_image(app: AppHandle) -> tauri::ipc::Response {
    let path = crate::paths::background_image_path(&app);
    match std::fs::read(path) {
        Ok(data) => tauri::ipc::Response::new(data),
        Err(_) => tauri::ipc::Response::new(Vec::new()),
    }
}

// ── toast 窗口 ────────────────────────────────────────────────

#[tauri::command]
pub fn validate_app_rule(
    window: tauri::WebviewWindow,
    rule: zhixin_core::app_rules::AppRule,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("仅设置窗口可以配置规则".into());
    }
    rule.validate()
}

#[tauri::command]
pub fn test_app_rule(
    window: tauri::WebviewWindow,
    rule: zhixin_core::app_rules::AppRule,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("仅设置窗口可以测试规则".into());
    }
    crate::app_launcher::open(&rule)
}

#[tauri::command]
pub fn toast_open_app(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    let app_id = app
        .state::<AppState>()
        .pending_toasts
        .lock()
        .unwrap()
        .get(window.label())
        .and_then(|payload| payload.app_id.clone())
        .ok_or("通知已过期或没有来源")?;
    let config = app.state::<AppState>().config();
    let rule =
        zhixin_core::app_rules::matching_rule(&config.notifications.app_rules, Some(&app_id))
            .ok_or("没有启用的打开规则")?;
    crate::app_launcher::open(rule)
}

/// toast 前端加载完成后主动来取自己的负载。
#[tauri::command]
pub fn get_toast_payload(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Option<ToastPayload> {
    state
        .pending_toasts
        .lock()
        .unwrap()
        .get(window.label())
        .cloned()
}

/// 用户点了 toast：有验证码就复制，返回复制了什么。
#[tauri::command]
pub fn toast_click(app: AppHandle, label: String, state: State<'_, AppState>) -> Option<String> {
    let code = state
        .pending_toasts
        .lock()
        .unwrap()
        .get(&label)
        .and_then(|payload| payload.code.clone());

    if let Some(code) = &code {
        if let Err(error) = app.clipboard().write_text(code.clone()) {
            warn!("复制验证码失败：{error}");
            return None;
        }
    }

    code
}

#[tauri::command]
pub fn toast_close(app: AppHandle, label: String) {
    popups::close_toast(&app, &label);
}

/// toast 前端量出了真实高度 → 调整窗口并显示。
#[tauri::command]
pub fn toast_resize(app: AppHandle, label: String, height: f64) {
    popups::resize_and_show(&app, &label, height);
}

// ── 候选条窗口 ────────────────────────────────────────────────

/// caret 前端加载完成后主动来取当前提议。
#[tauri::command]
pub fn get_caret_offer(
    state: State<'_, AppState>,
) -> Option<(u64, String, u32, Option<String>, u64)> {
    state.caret.lock().unwrap().offer.as_ref().map(|offer| {
        (
            offer.generation,
            offer.code.clone(),
            offer.duration_secs,
            offer.source_hint.clone(),
            offer.session_id,
        )
    })
}

#[tauri::command]
pub fn caret_layout(app: AppHandle, layout: CaretLayout) {
    caret::layout(&app, layout);
}

#[tauri::command]
pub async fn caret_insert(app: AppHandle, generation: u64) -> Option<(bool, String)> {
    tauri::async_runtime::spawn_blocking(move || caret::insert(&app, generation))
        .await
        .ok()
        .flatten()
}

#[tauri::command]
pub fn caret_hide(app: AppHandle, generation: u64) {
    caret::hide_if_generation(&app, generation);
}

#[tauri::command]
pub fn caret_dismiss(app: AppHandle, session: u64) {
    caret::dismiss(&app, session);
}

// ── 主窗口行为 ────────────────────────────────────────────────

#[tauri::command]
pub fn open_main_window(app: AppHandle) {
    tray::open_main_window(&app);
}
