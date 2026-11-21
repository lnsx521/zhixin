//! 知信 主程序。
//!
//! 组装：插件 → 全局状态 → 托盘 → 蓝牙链路线程 → 命令。

mod app_launcher;
mod caret;
mod commands;
mod devices;
mod history;
mod http_ingress;
mod icon_store;
mod link_handle;
mod link_worker;
mod mdns;
mod paths;
mod popups;
mod proximity;
mod state;
mod token_store;
mod transfer;
mod tray;
mod types;
mod updater;

use log::info;
use zhixin_core::config::Config;
use state::AppState;
use tauri::{Manager, WindowEvent};

pub fn run() {
    tauri::Builder::default()
        // 单实例必须最先注册。第二个实例启动时 → 把主窗口唤出来。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::open_main_window(app);
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("zhixin".to_string()),
                    }),
                ])
                .level(log::LevelFilter::Info)
                .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--silent"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updater::UpdateState::default())
        .setup(|app| {
            let first_run = !paths::config_path(app.handle()).exists();
            if first_run {
                let _ = Config::write_default_config(paths::config_path(app.handle()));
            }

            app.manage(AppState::new(app.handle()));
            app.manage(http_ingress::HttpIngress::load(app.handle()));
            http_ingress::start(app.handle().clone());

            tray::setup(app.handle())?;
            popups::ensure_pool(app.handle());
            link_worker::spawn(app.handle().clone());
            proximity::spawn(app.handle().clone());
            mdns::spawn(app.handle().clone());

            // 主窗口关到托盘，不退出
            if let Some(main_window) = app.get_webview_window("main") {
                let window_for_event = main_window.clone();
                main_window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window_for_event.hide();
                    }
                });

                // 手动双击启动 → 立即打开主窗口；开机自启（--silent）→ 静默托盘。
                // 旧版注册的自启动命令行没有 --silent 参数，这里刷新一次注册以补上。
                let silent_launch = std::env::args().any(|a| a == "--silent");
                if !silent_launch {
                    use tauri_plugin_autostart::ManagerExt;
                    if app.autolaunch().is_enabled().unwrap_or(false) {
                        let _ = app.autolaunch().enable();
                    }
                }

                // 首次运行或手动启动 → 打开主窗口（首次默认落在「连接引导」页）
                if first_run || !silent_launch {
                    let _ = main_window.show();
                    let _ = main_window.set_focus();
                }
            }

            // 首次安装：启动约 3 秒后自动投递开发者来信（一次性，靠标记文件去重，
            // 与配置/历史重置无关）。延后是为了等主窗口和弹窗池就绪，信能正常弹出。
            let welcome_flag = paths::welcome_sent_path(app.handle());
            if !welcome_flag.exists() {
                let _ = std::fs::write(&welcome_flag, b"1");
                let app_handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    commands::send_test_kind(app_handle, "mail");
                    info!("首次安装：已自动投递开发者来信");
                });
            }

            info!("知信 已启动");

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            http_ingress::get_http_status,
            http_ingress::list_http_addresses,
            http_ingress::open_firewall_rules,
            http_ingress::configure_http,
            http_ingress::reset_http_token,
            http_ingress::set_http_copy,
            updater::get_update_status,
            updater::check_update,
            updater::download_update,
            updater::install_update,
            updater::open_update_link,
            commands::save_config,
            commands::get_settings_meta,
            commands::set_autostart,
            popups::toast_expand,
            icon_store::get_app_icon,
            icon_store::list_icon_cache,
            icon_store::set_app_icon,
            icon_store::reset_app_icon,
            icon_store::clear_icon_cache,
            icon_store::open_icons_dir,
            history::get_call_stats,
            history::list_notification_history,
            history::clear_notification_history,
            history::export_notification_history,
            commands::open_config_dir,
            commands::open_logs_dir,
            commands::open_project_link,
            commands::open_bluetooth_settings,
            commands::get_link_state,
            commands::list_devices,
            commands::set_preferred_device,
            commands::set_device_enabled,
            commands::forget_device,
            commands::send_test_notification,
            commands::get_computer_name,
            popups::history_expand,
            commands::pick_transfer_folder,
            commands::open_app_by_id,
            popups::open_settings,
            commands::get_toast_payload,
            commands::toast_click,
            commands::toast_click_for_main,
            commands::toast_open_app,
            commands::test_app_rule,
            commands::validate_app_rule,
            commands::save_background_image,
            commands::get_background_image,
            commands::toast_close,
            commands::toast_resize,
            commands::get_caret_offer,
            commands::caret_layout,
            commands::caret_insert,
            commands::caret_hide,
            commands::caret_dismiss,
            commands::open_main_window,
            transfer::get_transfer_info,
            transfer::set_transfer_folder,
            transfer::open_transfer_folder,
            link_handle::reject_call,
            link_handle::answer_call,
            link_handle::hangup_call,
            http_ingress::reset_token,
            popups::pick_sound_file,
            mdns::get_mdns_hostname,
            popups::list_known_apps,
        ])
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                http_ingress::stop(app);
                app.state::<AppState>()
                    .link_stop
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
        });
}
