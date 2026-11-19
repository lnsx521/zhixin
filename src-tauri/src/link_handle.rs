//! 来电拒接的调用入口：把 ANCS 负面动作暴露给弹窗前端。

use crate::link_worker;
use std::sync::Arc;
use zhixin_ble::AncsLink;

/// 管理当前活跃的 ANCS 链路（由 link_worker 在蓝牙线程上创建并注册）。
pub struct LinkHandle {
    pub link: Arc<AncsLink>,
}

/// 拒接当前来电（按 ANCS 通知 UID 执行负面动作）。
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
    .map_err(|e| e.to_string())?
}
