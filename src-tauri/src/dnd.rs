//! 演示勿扰：检测全屏演示场景，询问用户是否静默弹窗。
//!
//! 三层信号：
//! * 系统级（`SHQueryUserNotificationState`）：D3D 独占全屏、系统演示模式——
//!   直接静默暂存不询问（独占全屏下弹窗会破坏全屏）；
//! * 确定窗口信号：PowerPoint 放映（前台类名 `screenClass`）→「正在全屏演示…」；
//! * 疑似窗口信号：无边框 + 覆盖整屏（含任务栏）+ 非最大化的前台窗口 →「疑似全屏演示…」。
//!
//! 静默跟随全屏：全屏退出持续 `dnd_recover_secs` 秒后恢复弹窗，并补送暂存通知。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use zhixin_core::model::PhoneNotification;

use crate::popups;

static DND_MODE: AtomicBool = AtomicBool::new(false); // 用户同意的静默
static DND_DECLINED: AtomicBool = AtomicBool::new(false); // 本会话已拒绝
static DND_ASKED: AtomicBool = AtomicBool::new(false); // 本会话已询问

/// 当前处于演示静默（用户同意，或系统级全屏）：通知只入暂存队列不弹窗。
pub fn silent() -> bool {
    DND_MODE.load(Ordering::Relaxed)
}

/// 用户在勿扰询问弹窗上的选择。
pub fn set_choice(accept: bool) {
    if accept {
        DND_MODE.store(true, Ordering::Relaxed);
    } else {
        DND_DECLINED.store(true, Ordering::Relaxed);
    }
}

/// 系统级全屏（独占全屏应用 / 系统演示模式）。保持既有行为：直接静默，不询问。
pub fn system_fullscreen() -> bool {
    use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUERY_USER_NOTIFICATION_STATE};
    unsafe {
        match SHQueryUserNotificationState() {
            Ok(state) => matches!(
                state,
                QUERY_USER_NOTIFICATION_STATE(2)
                    | QUERY_USER_NOTIFICATION_STATE(4)
                    | QUERY_USER_NOTIFICATION_STATE(6)
            ),
            Err(_) => false,
        }
    }
}

/// 前台窗口状态：`None` = 无全屏窗口；`Some(true)` = 确定是演示（PPT 放映）；`Some(false)` = 疑似。
fn window_fullscreen() -> Option<bool> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowTextW, GetWindowRect,
        IsZoomed, GWL_STYLE, WS_CAPTION,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        // 知信自己的窗口（主窗 / 弹窗 / 候选条）不参与判定
        let mut title = [0u16; 64];
        let len = GetWindowTextW(hwnd, &mut title);
        if String::from_utf16_lossy(&title[..len as usize]) == "知信" {
            return None;
        }
        // 最大化窗口：有标题栏且不覆盖任务栏，显式排除最稳
        if IsZoomed(hwnd).as_bool() {
            return None;
        }
        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        if style & WS_CAPTION.0 != 0 {
            return None;
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return None;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let m = info.rcMonitor;
        // 必须完整覆盖整个显示器（含任务栏区域）才算全屏
        if rect.left > m.left || rect.top > m.top || rect.right < m.right || rect.bottom < m.bottom
        {
            return None;
        }
        let mut class_buf = [0u16; 256];
        let class_len = GetClassNameW(hwnd, &mut class_buf);
        let class = String::from_utf16_lossy(&class_buf[..class_len as usize]);
        if matches!(
            class.as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        ) {
            return None;
        }
        // PowerPoint 放映：确定是演示
        Some(class == "screenClass")
    }
}

/// 演示勿扰主循环：检测、询问、静默、退出恢复与暂存补弹。
pub fn start_watch(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last_lost: Option<Instant> = None;
        let mut seen_fullscreen = false;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let window = window_fullscreen();
            let active = system_fullscreen() || window.is_some();

            if active {
                seen_fullscreen = true;
                last_lost = None;

                if system_fullscreen() {
                    // 独占全屏下弹窗会破坏全屏：保持既有行为，直接静默暂存
                    DND_MODE.store(true, Ordering::Relaxed);
                    continue;
                }

                let ask_enabled = {
                    let state = app.state::<crate::state::AppState>();
                    state.config().notifications.dnd_ask_enabled
                };
                if !ask_enabled
                    || DND_ASKED.load(Ordering::Relaxed)
                    || DND_MODE.load(Ordering::Relaxed)
                    || DND_DECLINED.load(Ordering::Relaxed)
                {
                    continue;
                }
                DND_ASKED.store(true, Ordering::Relaxed);
                let body = if window == Some(true) {
                    "正在全屏演示，是否开启勿扰模式？"
                } else {
                    "疑似全屏演示，是否开启勿扰模式？"
                };
                let ask = PhoneNotification {
                    device_id: "dnd".into(),
                    device_name: None,
                    uid: u32::MAX,
                    app_identifier: None,
                    title: Some("勿扰模式".into()),
                    subtitle: None,
                    message: Some(body.into()),
                    code: None,
                    category: Some(2),
                    call_outcome: None,
                    received_at: std::time::SystemTime::now(),
                };
                popups::show(&app, &ask);
                continue;
            }

            // 全屏已退出：静默状态保持 N 秒后复位，并补送暂存期间的通知
            if !seen_fullscreen {
                continue;
            }
            match last_lost {
                None => {
                    last_lost = Some(Instant::now());
                }
                Some(lost) => {
                    let recover = {
                        let state = app.state::<crate::state::AppState>();
                        state.config().notifications.dnd_recover_secs.clamp(5, 600) as u64
                    };
                    if lost.elapsed() >= Duration::from_secs(recover) {
                        DND_MODE.store(false, Ordering::Relaxed);
                        DND_DECLINED.store(false, Ordering::Relaxed);
                        DND_ASKED.store(false, Ordering::Relaxed);
                        seen_fullscreen = false;
                        let pending: Vec<PhoneNotification> = {
                            let state = app.state::<crate::state::AppState>();
                            let mut queue = state.deferred_notifications.lock().unwrap();
                            std::mem::take(&mut *queue)
                        };
                        for notification in pending {
                            popups::show(&app, &notification);
                        }
                    }
                }
            }
        }
    });
}
