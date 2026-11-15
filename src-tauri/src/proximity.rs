//! 感应锁屏：手机蓝牙断开超过设定时长后锁定 Windows。
//!
//! 信号源就是 知信 自己维护的 ANCS 链路状态（见 [`crate::link_worker`]）：
//! 手机在身边且连着 = `Subscribed`；人走远 / 手机关蓝牙 = 断开。
//!
//! 判定规则刻意保守：
//! - 只有「连上过至少一次」才武装，避免开机蓝牙未连接时把人锁在外面；
//! - 断开必须**持续**超过设定秒数才锁，短暂抖动不触发；
//! - 锁一次之后解除武装，等手机重新连上再武装，避免反复锁屏。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use log::{info, warn};
use zhixin_ble::LinkState;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// 轮询间隔。1 秒足够：人对「几秒后锁屏」的感知粒度也就到这了。
const TICK: Duration = Duration::from_secs(1);

/// 启动感应锁屏监督线程（独立线程，随链路线程的停止信号一起退出）。
pub fn spawn(app: AppHandle) {
    let stop = app.state::<AppState>().link_stop.clone();

    let result = std::thread::Builder::new()
        .name("proximity-lock".to_string())
        .spawn(move || run(app, stop));
    if let Err(error) = result {
        warn!("感应锁屏线程启动失败：{error}");
    }
}

fn run(app: AppHandle, stop: Arc<AtomicBool>) {
    // 是否见过手机连上（武装条件）。
    let mut armed = false;
    // 从「已订阅」掉下来的时刻；None = 当前连着或未武装。
    let mut not_ready_since: Option<Instant> = None;

    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(TICK);
        if stop.load(Ordering::Relaxed) {
            return;
        }

        let app_state = app.state::<AppState>();
        let (state, _detail) = app_state
            .link_state
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone());

        if state == LinkState::Subscribed {
            armed = true;
            not_ready_since = None;
            continue;
        }

        // 无论开关状态都持续维护 armed / not_ready_since；
        // 只在真正要锁屏时读配置，避免每秒一次配置锁。
        if !armed {
            not_ready_since = None;
            continue;
        }

        let since = match not_ready_since {
            Some(since) => since,
            None => {
                not_ready_since = Some(Instant::now());
                continue;
            }
        };

        let config = app_state.config();
        if !config.proximity.enabled {
            continue;
        }

        let delay_secs = config.proximity.lock_delay_secs.max(1) as u64;
        if since.elapsed() >= Duration::from_secs(delay_secs) {
            info!("手机断开超过 {delay_secs} 秒，执行感应锁屏");
            // SAFETY: LockWorkStation 只是锁定当前会话，无参数无状态，
            // 官方文档标注可从任意线程调用。
            if let Err(error) = unsafe { windows::Win32::System::Shutdown::LockWorkStation() } {
                warn!("锁屏调用失败：{error}");
            }
            // 锁完解除武装，重新连上之前不再触发。
            armed = false;
            not_ready_since = None;
        }
    }
}
