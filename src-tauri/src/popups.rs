//! 右下角通知弹窗（toast）的窗口管理。
//!
//! 每条通知是一个独立的小窗口：无边框、透明、置顶、不抢焦点。
//! 堆叠在屏幕右下角，新的在最下面，超上限先关最旧的。
//!
//! **高度自适应**：窗口先按默认高度隐藏创建，前端渲染完内容后量出真实高度
//! 调 `toast_resize` —— Rust 记录高度、重排堆叠、再把窗口显示出来。
//! 内容两行三行都不会被裁。

use std::sync::atomic::Ordering;
use std::time::Duration;

use log::{info, warn};
use zhixin_core::model::PhoneNotification;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Position, Size, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};

use crate::state::AppState;
use crate::types::ToastPayload;

/// 常驻弹窗池窗口的标签：应用启动时预建、关闭时隐藏不销毁，通知到达时热复用（零延迟）。
pub const POOL_LABEL: &str = "toast-pool";

/// 刚创建时的占位高度（DIP；马上会被前端报上来的真实高度替换）。
const DEFAULT_HEIGHT: f64 = 96.0;

/// 高度下限/上限（DIP）。上限防止超长通知占满屏幕。
const MIN_HEIGHT: f64 = 56.0;
const MAX_HEIGHT: f64 = 600.0;

const MARGIN: f64 = 16.0;
const GAP: f64 = 8.0;

#[derive(serde::Serialize, Clone)]
pub struct KnownApp {
    pub id: String,
    pub name: String,
}

/// 最近弹过通知的来源列表（设置页快捷选用）。
#[tauri::command]
pub fn list_known_apps(
    window: tauri::WebviewWindow,
    app: AppHandle,
) -> Result<Vec<KnownApp>, String> {
    if window.label() != "main" {
        return Err("Settings window required".into());
    }
    let known = app.state::<AppState>().known_apps.lock().unwrap().clone();
    Ok(known
        .into_iter()
        .map(|(id, name)| KnownApp { id, name })
        .collect())
}

/// 全屏勿扰：检测系统进入全屏（视频/投屏/游戏）时通知暂存，退出全屏后自动补弹。
fn is_fullscreen() -> bool {
    use windows::Win32::UI::Shell::SHQueryUserNotificationState;
    unsafe {
        match SHQueryUserNotificationState() {
            Ok(state) => matches!(
                state,
                windows::Win32::UI::Shell::QUERY_USER_NOTIFICATION_STATE(2)
                    | windows::Win32::UI::Shell::QUERY_USER_NOTIFICATION_STATE(4)
                    | windows::Win32::UI::Shell::QUERY_USER_NOTIFICATION_STATE(6)
            ),
            Err(_) => false,
        }
    }
}

/// 全屏退出轮询：每 2 秒检查一次，退出全屏后按序补弹暂存通知。
pub fn start_fullscreen_watch(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(2));
        if is_fullscreen() {
            continue;
        }
        let pending: Vec<PhoneNotification> = {
            let state = app.state::<AppState>();
            let mut queue = state.deferred_notifications.lock().unwrap();
            std::mem::take(&mut *queue)
        };
        for notification in pending {
            show(&app, &notification);
        }
    });
}

/// 预建常驻弹窗池窗口（隐藏、无负载）。前端发现自己是池窗口且无负载时保持隐藏待命。
pub fn ensure_pool(app: &AppHandle) {
    if app.get_webview_window(POOL_LABEL).is_some() {
        return;
    }
    let state = app.state::<AppState>();
    state
        .toast_heights
        .lock()
        .unwrap()
        .insert(POOL_LABEL.to_string(), DEFAULT_HEIGHT);

    let builder = WebviewWindowBuilder::new(app, POOL_LABEL, WebviewUrl::App("toast.html".into()))
        .title("知信")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focusable(false)
        .focused(false)
        .resizable(false)
        .inner_size(state.config().notifications.popup.width, DEFAULT_HEIGHT)
        .visible(false);

    if let Err(error) = builder.build() {
        warn!("弹窗池预建失败：{error}");
        state.toast_heights.lock().unwrap().remove(POOL_LABEL);
    }
}

/// 弹一条通知。
pub fn show(app: &AppHandle, notification: &PhoneNotification) {
    if is_fullscreen() {
        let state = app.state::<AppState>();
        state.deferred_notifications.lock().unwrap().push(notification.clone());
        info!("全屏勿扰：通知暂存（{} 条待补弹）", state.deferred_notifications.lock().unwrap().len());
        return;
    }
    let state = app.state::<AppState>();
    let config = state.config();
    let popup = &config.notifications.popup;

    // 超上限先关最旧的
    {
        let open: Vec<String> = state.toasts.lock().unwrap().iter().cloned().collect();
        let excess = open
            .len()
            .saturating_add(1)
            .saturating_sub(popup.max_visible.max(1));
        for label in open.into_iter().take(excess) {
            close_toast(app, &label);
        }
    }

    let id = state.toast_seq.fetch_add(1, Ordering::Relaxed);
    let label = format!("toast-{id}");

    let payload = ToastPayload {
        app_id: notification.app_identifier.clone(),
        open_app_name: zhixin_core::app_rules::matching_rule(
            &config.notifications.app_rules,
            notification.app_identifier.as_deref(),
        )
        .map(|rule| rule.name.clone()),
        origin: notification.origin(),
        body: notification.summary(),
        code: notification.code.clone(),
        duration_secs: popup.duration_seconds,
        category: notification.category,
        uid: notification.uid,
        carrier: if notification.category == Some(1) {
            zhixin_core::carrier::describe(&origin_digits(&notification.origin()))
        } else {
            None
        },
    };

    info!("弹窗：{} — {}", payload.origin, payload.body);

    // 弹窗池热复用：常驻窗口空闲时直接换内容显示（零延迟，免 webview 冷加载）
    {
        let pool_free = app.get_webview_window(POOL_LABEL).is_some()
            && !state
                .toasts
                .lock()
                .unwrap()
                .iter()
                .any(|item| item == POOL_LABEL);
        if pool_free {
            state
                .pending_toasts
                .lock()
                .unwrap()
                .insert(POOL_LABEL.to_string(), payload);
            state
                .toasts
                .lock()
                .unwrap()
                .push_back(POOL_LABEL.to_string());
            state
                .toast_heights
                .lock()
                .unwrap()
                .insert(POOL_LABEL.to_string(), DEFAULT_HEIGHT);
            let _ = app.emit_to(POOL_LABEL, "toast-payload", ());
            reposition_all(app);
            let duration = u64::from(popup.duration_seconds).max(2) + 2;
            let app_for_timer = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(duration)).await;
                close_toast(&app_for_timer, POOL_LABEL);
            });
            return;
        }
    }

    // 通知历史留档（含验证码，敏感数据仅本地存储）
    crate::history::record(app, notification);

    // 联网获取 App 图标（开关打开时）：后台静默，失败不影响通知
    if let Some(app_id) = payload.app_id.clone().filter(|id| !id.trim().is_empty()) {
        crate::icon_store::spawn_fetch(app.clone(), app_id, payload.origin.clone());
    }

    // 记住来源（过滤规则页可直接选用），最新在前、去重、上限 50。
    if let Some(app_id) = payload.app_id.clone().filter(|id| !id.trim().is_empty()) {
        let mut known = state.known_apps.lock().unwrap();
        known.retain(|(id, _)| id != &app_id);
        known.insert(0, (app_id, payload.origin.clone()));
        known.truncate(50);
    }

    state
        .pending_toasts
        .lock()
        .unwrap()
        .insert(label.clone(), payload);
    state
        .toast_heights
        .lock()
        .unwrap()
        .insert(label.clone(), DEFAULT_HEIGHT);

    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("toast.html".into()))
        .title("知信")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        // ★ 不抢焦点：弹窗出现 / 被点击都不能动用户的前台窗口。
        .focusable(false)
        .focused(false)
        .resizable(false)
        .inner_size(popup.width, DEFAULT_HEIGHT)
        .visible(false);

    let window = match builder.build() {
        Ok(window) => window,
        Err(error) => {
            warn!("创建 toast 窗口失败：{error}");
            state.pending_toasts.lock().unwrap().remove(&label);
            state.toast_heights.lock().unwrap().remove(&label);
            return;
        }
    };

    {
        let mut open = state.toasts.lock().unwrap();
        open.push_back(label.clone());
    }

    // 窗口销毁（不管哪条路关的）→ 从堆里摘掉并重排
    let app_for_event = app.clone();
    let label_for_event = label.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            on_toast_destroyed(&app_for_event, &label_for_event);
        }
    });

    // 先按占位高度摆好，但**不显示** —— 等前端报真实高度后再 show（见 resize_and_show）
    reposition_all(app);

    // 兜底定时关闭（前端崩溃/事件丢失也不至于留着窗口）
    let app_for_timer = app.clone();
    let label_for_timer = label.clone();
    let duration = u64::from(popup.duration_seconds).max(2) + 2;
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(duration)).await;
        close_toast(&app_for_timer, &label_for_timer);
    });
}

/// 点小弹窗展开 → 关小窗，开一个 Mac 风格大窗展示全文。
#[tauri::command]
pub async fn toast_expand(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    // ★ 必须异步：同步命令在主线程执行，而窗口创建要派发回主线程并等待——同步执行会自死锁。
    tauri::async_runtime::spawn_blocking(move || toast_expand_impl(app, window))
        .await
        .map_err(|e| e.to_string())?
}

fn toast_expand_impl(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    let small_label = window.label().to_string();
    let state = app.state::<AppState>();
    info!("展开请求：{}（展开窗口开始创建）", small_label);

    let Some(payload) = state.pending_toasts.lock().unwrap().get(&small_label).cloned() else {
        warn!("展开失败：{} 的负载已不存在", small_label);
        return Err("payload gone".into());
    };

    // 同一时刻只允许一个大窗：已存在则关闭旧的再开新的
    {
        let open = state.toasts.lock().unwrap().clone();
        for stale in open.iter().filter(|l| l.starts_with("expanded-")) {
            close_toast(&app, stale);
        }
    }

    let id = state.toast_seq.fetch_add(1, Ordering::Relaxed);
    let label = format!("expanded-{id}");
    state
        .pending_toasts
        .lock()
        .unwrap()
        .insert(label.clone(), payload);

    let width = 560.0;
    let mon0 = app.primary_monitor().ok().flatten();
    let scale0 = mon0.as_ref().map(|m| m.scale_factor()).unwrap_or(1.0);
    let height = match mon0.as_ref() {
        Some(m) => ((m.work_area().size.height as f64) * 0.76 / scale0).clamp(420.0, 900.0),
        None => 700.0,
    };
    let builder = WebviewWindowBuilder::new(
        &app,
        &label,
        WebviewUrl::App("expanded.html".into()),
    )
    .title("知信")
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .inner_size(width, height)
    .visible(false);

    let window = match builder.build() {
        Ok(window) => window,
        Err(error) => {
            warn!("展开窗口创建失败：{error}");
            state.pending_toasts.lock().unwrap().remove(&label);
            return Err(error.to_string());
        }
    };
    info!("展开窗口已创建：{}", label);

    // 大窗销毁 → 清理负载
    let app_for_event = app.clone();
    let label_for_event = label.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            app_for_event
                .state::<AppState>()
                .pending_toasts
                .lock()
                .unwrap()
                .remove(&label_for_event);
        }
    });

    // 居中显示（高度等前端量完报上来再调）
    if let Some(monitor) = app.primary_monitor().ok().flatten() {
        let scale = window.scale_factor().unwrap_or(1.0);
        let work = monitor.work_area();
        let _ = window.set_position(Position::Physical(PhysicalPosition::new(
            work.position.x + ((work.size.width as f64 - width * scale) / 2.0).ceil() as i32,
            work.position.y + ((work.size.height as f64 - height * scale) / 2.8).ceil() as i32,
        )));
    }

    // 关掉小弹窗（负载已迁移，大窗接管展示）
    close_toast(&app, &small_label);
    Ok(())
}

/// 选择自定义提示音（wav 文件）。
#[tauri::command]
pub async fn pick_sound_file(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .add_filter("WAV 音频", &["wav"])
        .pick_file(move |path| {
            let _ = tx.send(path.map(|p| p.to_string()));
        });
    Ok(rx.await.map_err(|e| e.to_string())?)
}

/// 从文本里提取最长连续数字串（号码识别用）。
fn origin_digits(text: &str) -> String {
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

/// 前端量出了真实高度 → 记高度、改尺寸、重排、显示。
pub fn resize_and_show(app: &AppHandle, label: &str, height_dip: f64) {
    if app
        .state::<crate::state::AppState>()
        .config()
        .notifications
        .popup
        .sound
    {
        let sound_file = app
            .state::<crate::state::AppState>()
            .config()
            .notifications
            .popup
            .sound_file
            .clone();
        let custom = sound_file.filter(|p| !p.trim().is_empty()).filter(|p| std::path::Path::new(p).is_file());
        if let Some(path) = custom {
            use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME};
            use windows::core::HSTRING;
            unsafe {
                let _ = PlaySoundW(&HSTRING::from(path.as_str()), None, SND_FILENAME | SND_ASYNC);
            }
        } else {
            use windows::Win32::System::Diagnostics::Debug::MessageBeep;
            use windows::Win32::UI::WindowsAndMessaging::MB_ICONASTERISK;
            unsafe { let _ = MessageBeep(MB_ICONASTERISK); }
        }
    }
    // 高度始终按前端量出的内容自适应（用户不可自调）。
    // 大窗（长文阅读）不套小窗上限，按工作区 88% 封顶
    let state = app.state::<AppState>();
    let (width, height) = if label.starts_with("expanded-") {
        let m0 = app.primary_monitor().ok().flatten();
        let w_scale = m0.as_ref().map(|m| m.scale_factor()).unwrap_or(1.0);
        let w_h = m0.map(|m| m.work_area().size.height as f64).unwrap_or(1000.0);
        (560.0f64, ((w_h * 0.76) / w_scale).clamp(420.0, 900.0))
    } else {
        (state.config().notifications.popup.width, height_dip.clamp(MIN_HEIGHT, MAX_HEIGHT))
    };

    state
        .toast_heights
        .lock()
        .unwrap()
        .insert(label.to_string(), height);

    let Some(window) = app.get_webview_window(label) else {
        return;
    };

    let scale = window.scale_factor().unwrap_or(1.0);
    let _ = window.set_size(Size::Physical(PhysicalSize::new(
        (width * scale).ceil() as u32,
        (height * scale).ceil() as u32,
    )));

    reposition_all(app);
    let _ = window.show();
}

/// 关掉一个 toast（用户点掉 / 超时 / 被挤掉）。
pub fn close_toast(app: &AppHandle, label: &str) {
    // 池窗口只隐藏不销毁 —— 保留 webview 热状态，下一条通知零延迟复用
    if label == POOL_LABEL {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.hide();
        }
        on_toast_destroyed(app, label);
        return;
    }
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.close();
    } else {
        // 窗口已经没了，补一次清理
        on_toast_destroyed(app, label);
    }
}

fn on_toast_destroyed(app: &AppHandle, label: &str) {
    let state = app.state::<AppState>();

    let mut open = state.toasts.lock().unwrap();
    let removed = open
        .iter()
        .position(|item| item == label)
        .map(|index| open.remove(index));
    state.pending_toasts.lock().unwrap().remove(label);
    state.toast_heights.lock().unwrap().remove(label);
    drop(open);

    if removed.is_some() {
        reposition_all(app);
    }
}

/// 按「最新的在最下面」重排所有 toast（高度各算各的）。
fn reposition_all(app: &AppHandle) {
    let state = app.state::<AppState>();
    let config = state.config();

    let Some(monitor) = app.primary_monitor().ok().flatten() else {
        return;
    };

    let work_area = monitor.work_area();
    let scale = monitor.scale_factor();
    let width_px = (config.notifications.popup.width * scale) as i32;
    let margin_px = (MARGIN * scale) as i32;
    let gap_px = (GAP * scale) as i32;

    let open = state.toasts.lock().unwrap();
    let heights = state.toast_heights.lock().unwrap();

    // macOS 式右上角堆叠：最新的贴顶，旧的依次往下
    let mut top = work_area.position.y + margin_px;

    for label in open.iter().rev() {
        let height_px = (heights.get(label).copied().unwrap_or(DEFAULT_HEIGHT) * scale) as i32;
        let x = work_area.position.x + work_area.size.width as i32 - width_px - margin_px;
        let y = top;

        if let Some(window) = app.get_webview_window(label) {
            let _ = window.set_position(Position::Physical(PhysicalPosition::new(x, y)));
        }

        top = y + height_px + gap_px;
    }
}
