//! mDNS 域名广播：手机端用一个固定域名访问电脑，换网络不用换地址。
//!
//! 发布 `_zhixin._tcp.local.` 服务，主机名为 `<机器名>-<指纹>.local.`：
//! - 机器名做主标签，人能认出来是谁的电脑；
//! - 机器名 + 用户名的哈希取 4 位指纹防同事之间同名撞车；
//! - IP 变化由 mdns-sd 的 `enable_addr_auto` 自动跟随，换 Wi-Fi 即刻生效。
//!
//! 服务跟随 HTTP 接收器开关：接收器开着才注册（端口也跟它走），
//! 每 5 秒对一次账，端口变了就换发。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use log::{info, warn};
use mdns_sd::{ServiceDaemon, ServiceInfo};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// 对账间隔：接收器开关 / 改端口后至多 5 秒内跟上。
const TICK: Duration = Duration::from_secs(5);

/// mDNS 单个标签上限（规范 63 字节，留余量给指纹尾巴）。
const LABEL_MAX: usize = 60;

/// 域名（形如 `huaweimatebookp-9f3a`，不带 `.local` 后缀）。进程内恒定。
static HOST_LABEL: OnceLock<String> = OnceLock::new();

/// 广播的服务类型名。
const SERVICE_TYPE: &str = "_zhixin._tcp.local.";

/// 当前域名标签；带 `.local` 后缀的完整域名用 [`hostname`]。
pub fn host_label() -> &'static str {
    HOST_LABEL.get_or_init(generate_label)
}

/// 完整域名（`xxx.local`）。永远可用，与接收器是否开启无关。
pub fn hostname() -> String {
    format!("{}.local", host_label())
}

fn generate_label() -> String {
    let machine = std::env::var("COMPUTERNAME").unwrap_or_default();
    let user = std::env::var("USERNAME").unwrap_or_default();

    // 机器名转合法 mDNS 标签：小写，字母数字以外的字符压成连字符。
    let mut base: String = machine
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    // 连续连字符压成一个，去首尾。
    let mut squeezed = String::new();
    for c in base.chars() {
        if c == '-' && squeezed.ends_with('-') {
            continue;
        }
        squeezed.push(c);
    }
    base = squeezed.trim_matches('-').to_string();
    if base.is_empty() {
        base = "zhixin".to_string();
    }

    // 指纹：哈希非安全用途，仅用于区分同名机器。
    let mut hasher = Sha256::new();
    hasher.update(machine.as_bytes());
    hasher.update(user.as_bytes());
    let digest = hasher.finalize();
    let fingerprint: String = digest.iter().take(2).map(|b| format!("{b:02x}")).collect();

    // 截短机器名给指纹让位（标签上限 63）。
    let keep = LABEL_MAX
        .saturating_sub(fingerprint.len() + 1)
        .min(base.len());
    let base = &base[..keep];
    format!("{base}-{fingerprint}")
}

/// 启动域名监督线程（独立线程，随停止信号退出）。
pub fn spawn(app: AppHandle) {
    let stop = app.state::<AppState>().link_stop.clone();
    let label = host_label().to_string();
    info!("mDNS 域名：{label}.local");

    let result = std::thread::Builder::new()
        .name("mdns-broadcast".to_string())
        .spawn(move || run(app, stop, label));
    if let Err(error) = result {
        warn!("mDNS 广播线程启动失败：{error}");
    }
}

fn run(app: AppHandle, stop: Arc<AtomicBool>, label: String) {
    // 常驻 daemon；服务注册跟着接收器端口走。
    let mut daemon: Option<ServiceDaemon> = None;
    // 当前已注册服务对应的端口；None = 未注册（含「注册失败待重试」）。
    let mut registered: Option<u16> = None;

    loop {
        if stop.load(Ordering::Relaxed) {
            if let Some(d) = daemon {
                let _ = d.shutdown();
            }
            return;
        }
        std::thread::sleep(TICK);
        if stop.load(Ordering::Relaxed) {
            if let Some(d) = daemon {
                let _ = d.shutdown();
            }
            return;
        }

        let endpoint = app
            .state::<crate::http_ingress::HttpIngress>()
            .running_endpoint();
        let want = endpoint.as_ref().map(|(_, port, _)| *port);

        match (want, registered) {
            (None, None) => continue,                 // 一直没开，继续等
            (Some(a), Some(b)) if a == b => continue, // 无变化
            _ => {}
        }

        // 到这里 = 要么刚开（注册），要么端口变了（先注销再注册），要么刚关（注销）。
        if let (Some(d), Some(port)) = (daemon.as_ref(), registered) {
            let fullname = format!("{label}.{SERVICE_TYPE}");
            if let Err(error) = d.unregister(&fullname) {
                warn!("mDNS 注销失败：{error}");
            }
            info!("mDNS 已注销域名服务（原端口 {port}）");
        }
        registered = None;

        let Some(port) = want else { continue };

        if daemon.is_none() {
            match ServiceDaemon::new() {
                Ok(d) => daemon = Some(d),
                Err(error) => {
                    warn!("mDNS daemon 创建失败：{error}");
                    return; // 连 daemon 都起不来，重试无意义
                }
            }
        }
        let Some(d) = daemon.as_ref() else { continue };

        // 注册服务：实例名 = 域名标签，主机名 = <标签>.local.
        let host = format!("{label}.local.");
        let addresses = my_addresses();
        let version = env!("CARGO_PKG_VERSION").to_string();
        let properties: std::collections::HashMap<String, String> =
            [("version".to_string(), version)].into_iter().collect();
        let info = match ServiceInfo::new(
            SERVICE_TYPE,
            &label,
            &host,
            &addresses[..],
            port,
            properties,
        ) {
            Ok(info) => info.enable_addr_auto(),
            Err(error) => {
                warn!("mDNS 服务信息构造失败：{error}");
                continue;
            }
        };

        match d.register(info) {
            Ok(()) => {
                registered = Some(port);
                info!("mDNS 已广播 {label}.local → 端口 {port}");
            }
            Err(error) => {
                warn!("mDNS 注册失败（5 秒后重试）：{error}");
                // registered 保持 None，下一轮重试
            }
        }
    }
}

/// 当下所有活动 IPv4（含回环剔除），交给 enable_addr_auto 做初始地址。
fn my_addresses() -> Vec<std::net::IpAddr> {
    if_addrs::get_if_addrs()
        .map(|addrs| {
            addrs
                .into_iter()
                .map(|interface| interface.ip())
                .filter(|ip| ip.is_ipv4())
                .collect()
        })
        .unwrap_or_default()
}

// ── 设置命令 ──────────────────────────────

#[tauri::command]
pub fn get_mdns_hostname(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    if window.label() != "main" {
        return Err("Settings window required".into());
    }
    Ok(Some(hostname()))
}
