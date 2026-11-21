//! 来电拒接的调用入口：把 ANCS 负面动作暴露给弹窗前端。

use crate::link_worker;
use std::sync::Arc;
use zhixin_ble::AncsLink;

/// 管理当前活跃的 ANCS 链路（由 link_worker 在蓝牙线程上创建并注册）。
pub struct LinkHandle {
    pub link: Arc<AncsLink>,
}

/// 拒接当前来电（按 ANCS 通知 UID 执行负面动作），成功后写入历史。
#[tauri::command]
pub async fn reject_call(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    uid: u32,
) -> Result<(), String> {
    let _ = window;
    use tauri::Manager;
    let link = app
        .try_state::<LinkHandle>()
        .map(|h| h.inner().link.clone())
        .ok_or_else(|| "蓝牙链路未就绪".to_string())?;
    // ANCS 写入要尽快发——丢到阻塞线程池，避免卡弹窗 UI。
    tauri::async_runtime::spawn_blocking(move || {
        link.perform_negative_action(uid).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    // 拒接成功：立即往历史写一条"已拒接"的记录。
    // ANCS Removed 事件会被跳过，所以必须在这里主动入库。
    let state = app.state::<crate::state::AppState>();
    let mut locked = state.phone_notifs.lock().unwrap();
    if let Some(notification) = locked.values().find(|n| n.uid == uid && n.is_call()) {
        use zhixin_core::model::PhoneNotification;
        let rejected = PhoneNotification {
            call_outcome: Some("rejected".to_string()),
            ..notification.clone()
        };
        drop(locked);
        crate::history::record(&app, &rejected);
    } else {
        drop(locked);
    }

    Ok(())
}

/// 挂断「当前通话」（category=12 的通知 UID 仍有效，负向动作可真挂断）。
/// 与 reject_call 的区别：不写"已拒接"历史 —— 时长由会话层 Removed 统一写"已接通 N秒"，
/// 避免同一次通话既记"已拒接"又记"已接通"。
#[tauri::command]
pub async fn hangup_call(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    uid: u32,
) -> Result<(), String> {
    let _ = window;
    use tauri::Manager;
    let link = app
        .try_state::<LinkHandle>()
        .map(|h| h.inner().link.clone())
        .ok_or_else(|| "蓝牙链路未就绪".to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        link.perform_negative_action(uid).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(())
}

/// 接听当前来电（按 ANCS 通知 UID 执行正面动作）。
/// 接通后 iOS 推 Removed 事件，通话时长由会话层估算入库（绿药丸"已接通 · N秒"）。
#[tauri::command]
pub async fn answer_call(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    uid: u32,
) -> Result<(), String> {
    let _ = window;
    use tauri::Manager;
    let link = app
        .try_state::<LinkHandle>()
        .map(|h| h.inner().link.clone())
        .ok_or_else(|| "蓝牙链路未就绪".to_string())?;
    let _ = tauri::async_runtime::spawn_blocking(move || {
        link.perform_positive_action(uid).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}
