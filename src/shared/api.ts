/**
 * 与 Rust 后端的全部交互：invoke 命令 + 事件订阅。
 * 类型与 src-tauri/src/types.rs 里的 serde 结构保持一致。
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

// ── 配置（与 zhixin-core 的 Config 对应） ─────────────────────

export interface FilterOptions {
  exclude_apps: string[];
  include_apps: string[];
  exclude_keywords: string[];
  include_keywords: string[];
}

export interface PopupOptions {
  duration_seconds: number;
  max_visible: number;
  width: number;
  /** 弹窗高度（DIP），0 = 按内容自动 */
  height: number;
  /** 弹出通知时播放系统提示音 */
  sound: boolean;
}

export interface NotificationOptions {
  app_rules: AppRule[];
  enabled: boolean;
  filter: FilterOptions;
  popup: PopupOptions;
}

export interface AppRule {
  app_id: string;
  name: string;
  enabled: boolean;
  target: string;
}
export const testAppRule = (rule: AppRule) => invoke<void>("test_app_rule", { rule });
export const validateAppRule = (rule: AppRule) => invoke<void>("validate_app_rule", { rule });
export const toastOpenApp = () => invoke<void>("toast_open_app");

export interface CaretOptions {
  enabled: boolean;
  duration_seconds: number;
  watch_seconds: number;
  gap: number;
}

export type InsertMode = "direct" | "simulate";
export type ThemeSetting = "auto" | "light" | "dark";

export interface InsertionOptions {
  mode: InsertMode;
  type_delay_ms: number;
}

export interface OtpOptions {
  enabled: boolean;
  auto_copy: boolean;
  auto_fill: boolean;
  caret: CaretOptions;
  insertion: InsertionOptions;
}

export interface Config {
  notifications: NotificationOptions;
  otp: OtpOptions;
  general: {
    language: string; // "auto" | "zh" | "en"
    theme: ThemeSetting;
    background: string; // "classic" | "blue" | "warm" | "mint" | "custom"
    low_battery_notify: boolean;
    low_battery_threshold: number;
    icon_fetch: boolean;
  };
  proximity: {
    enabled: boolean; // 感应锁屏总开关
    lock_delay_secs: number; // 手机断开超过该秒数才锁屏
  };
}

export const getConfig = () => invoke<Config>("get_config");
export const saveConfig = (config: Config) => invoke<void>("save_config", { config });
export const onThemeChanged = (handler: (theme: ThemeSetting) => void) =>
  listen<ThemeSetting>("theme-changed", (event) => handler(event.payload));

// ── 元信息 ────────────────────────────────────────────────────

export interface SettingsMeta {
  version: string;
  config_path: string;
  log_dir: string;
  autostart_enabled: boolean;
}

export const getSettingsMeta = () => invoke<SettingsMeta>("get_settings_meta");
export const setAutostart = (enabled: boolean) =>
  invoke<boolean>("set_autostart", { enabled });
export const openConfigDir = () => invoke<void>("open_config_dir");
export const openLogsDir = () => invoke<void>("open_logs_dir");
export interface IconCacheEntry { app_id: string; name: string; manual: boolean }
export const listIconCache = () => invoke<IconCacheEntry[]>("list_icon_cache");
export const getAppIcon = (appId: string) => invoke<string | null>("get_app_icon", { appId });
export const setAppIcon = (appId: string, name: string, dataUrl: string) =>
  invoke<void>("set_app_icon", { appId, name, dataUrl });
export const resetAppIcon = (appId: string) => invoke<void>("reset_app_icon", { appId });
export const clearIconCache = () => invoke<void>("clear_icon_cache");
export const openIconsDir = () => invoke<void>("open_icons_dir");
export interface HistoryEntry {
  received_at: number;
  app_id: string | null;
  origin: string;
  body: string;
  code: string | null;
  category: number | null;
}
export const listNotificationHistory = (query: string, limit: number) =>
  invoke<HistoryEntry[]>("list_notification_history", { query, limit });
export const clearNotificationHistory = () => invoke<void>("clear_notification_history");
export const exportNotificationHistory = () => invoke<string>("export_notification_history");
export const openBluetoothSettings = () => invoke<void>("open_bluetooth_settings");
export type ProjectLink = "repository" | "maintainer" | "contributors" | "issues" | "author";
export const openProjectLink = (target: ProjectLink) => invoke<void>("open_project_link", { target });

export interface HttpDevice { id: string; name: string; allow_copy: boolean; last_received: number | null }
export interface HttpStatus { token: string | null; settings: { enabled: boolean; bind: string; port: number; allow_copy: boolean; devices: HttpDevice[] }; running: boolean; error: string | null }
export const resetHttpToken = () => invoke<void>("reset_http_token");
export const setHttpCopy = (allowCopy: boolean) => invoke<void>("set_http_copy", { allowCopy });
export const getHttpStatus = () => invoke<HttpStatus>("get_http_status");
export interface LocalAddress { name: string; address: string }
export const listHttpAddresses = () => invoke<LocalAddress[]>("list_http_addresses");
export const openFirewallRules = () => invoke<void>("open_firewall_rules");
export const configureHttp = (enabled: boolean, bind: string, port: number) => invoke<void>("configure_http", { enabled, bind, port });

// ── 文件互传 ─────────────────────────────────────────────────
export interface TransferInfo { enabled: boolean; port: number; token: string | null; folder: string }
export const getTransferInfo = () => invoke<TransferInfo>("get_transfer_info");
export const setTransferFolder = (folder: string) => invoke<string>("set_transfer_folder", { folder });
export const openTransferFolder = () => invoke<void>("open_transfer_folder");

// ── mDNS 域名 ────────────────────────────────────────────────
export const saveBackgroundImage = (data: Uint8Array) => invoke<void>("save_background_image", { data });
export const getBackgroundImage = () => invoke<ArrayBuffer>("get_background_image");

export const getMdnsHostname = () => invoke<string | null>("get_mdns_hostname");

export interface UpdateStatus {
  revision: number;
  phase: "disabled" | "idle" | "checking" | "latest" | "available" | "downloading" | "ready" | "installing" | "check_error" | "download_error" | "install_error";
  version: string | null;
  notes: string | null;
  downloaded: number;
  total: number | null;
  error: string | null;
  retry_attempt: number;
  last_check_at: number | null;
  next_retry_at: number | null;
}
export const getUpdateStatus = () => invoke<UpdateStatus>("get_update_status");
export const checkUpdate = () => invoke<void>("check_update");
export const downloadUpdate = () => invoke<void>("download_update");
export const installUpdate = () => invoke<void>("install_update");
export const openUpdateLink = (url: string) => invoke<void>("open_update_link", { url });
export const onUpdateStatus = (handler: (status: UpdateStatus) => void) =>
  listen<UpdateStatus>("update-status", (event) => handler(event.payload));

// ── 链路状态 ──────────────────────────────────────────────────

export interface LinkStatePayload {
  state: string;
  label: string;
  detail: string | null;
  ready: boolean;
  awaiting_verification: boolean;
}

export const getLinkState = () => invoke<LinkStatePayload>("get_link_state");
export const onLinkState = (handler: (payload: LinkStatePayload) => void) =>
  listen<LinkStatePayload>("link-state", (event) => handler(event.payload));

export const sendTestNotification = () => invoke<void>("send_test_notification");

export interface ManagedDevice {
  id: string;
  name: string;
  address_hint: string;
  preferred: boolean;
  enabled: boolean;
  current: boolean;
  connected: boolean;
  online: boolean;
  verified: boolean;
  last_connected_at: number | null;
  last_verified_at: number | null;
  battery_level: number | null;
}

export const listDevices = () => invoke<ManagedDevice[]>("list_devices");
export const setPreferredDevice = (id: string) =>
  invoke<void>("set_preferred_device", { id });
export const setDeviceEnabled = (id: string, enabled: boolean) =>
  invoke<void>("set_device_enabled", { id, enabled });
export const forgetDevice = (id: string) => invoke<void>("forget_device", { id });
export const onDevicesChanged = (handler: (devices: ManagedDevice[]) => void) =>
  listen<ManagedDevice[]>("devices-changed", (event) => handler(event.payload));

// ── toast 窗口 ────────────────────────────────────────────────

export interface ToastPayload {
  app_id: string | null;
  open_app_name: string | null;
  origin: string;
  body: string;
  code: string | null;
  duration_secs: number;
  /** iOS 通知分类：1 = 来电 */
  category: number | null;
}

export const getToastPayload = () => invoke<ToastPayload | null>("get_toast_payload");
export interface KnownApp { id: string; name: string }
export const listKnownApps = () => invoke<KnownApp[]>("list_known_apps");
export const toastClick = () =>
  invoke<string | null>("toast_click", { label: getCurrentWindow().label });
export const toastExpand = () =>
  invoke<void>("toast_expand");
export const toastClose = () =>
  invoke<void>("toast_close", { label: getCurrentWindow().label });
export const toastResize = (height: number) =>
  invoke<void>("toast_resize", { label: getCurrentWindow().label, height });

// ── 候选条窗口 ────────────────────────────────────────────────

/** [offerGeneration, code, durationSecs, sourceHint, sessionId] */
export type CaretOfferTuple = [number, string, number, string | null, number];

export const getCaretOffer = () => invoke<CaretOfferTuple | null>("get_caret_offer");
export const caretLayout = (generation: number, width: number, height: number) =>
  invoke<void>("caret_layout", { layout: { generation, width, height } });
export const caretInsert = (generation: number) =>
  invoke<[boolean, string] | null>("caret_insert", { generation });
export const caretHide = (generation: number) => invoke<void>("caret_hide", { generation });
export const caretDismiss = (session: number) => invoke<void>("caret_dismiss", { session });
export const onCaretOffer = (handler: () => void) =>
  listen<number>("caret-offer", () => handler());
