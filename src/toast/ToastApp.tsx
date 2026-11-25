import { useEffect, useRef, useState } from "react";
import {
  getConfig,
  getAppIcon,
  dndChoice,
  updateAskInstall,
  getToastPayload,
  toastClickForMain,
  toastClose,
  toastResize,
  type ToastPayload,
} from "../shared/api";
import { dict, resolveLang, type Lang } from "../shared/i18n";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useSavedTheme } from "../shared/theme";
import { brandAsset } from "../shared/brandIcons";

/**
 * 通知弹窗（小窗与 Mac 风格大窗共用）。
 *
 * 负载不由事件下发（事件可能早于页面加载），而是页面挂载后主动向 Rust 取。
 * 小窗：内容最多 3 行，超出显示「查看全文」；点卡片任意处（除按钮）→ 展开成大窗。
 * 大窗（mode=expanded）：全文展示、无倒计时；✕ 关闭。
 */

export function ToastApp({ mode = "small" }: { mode?: "small" | "expanded" }) {
  const expanded = mode === "expanded";
  useSavedTheme();
  const [payload, setPayload] = useState<ToastPayload | null>(null);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lang, setLang] = useState<Lang>("zh");
  const [cachedIcon, setCachedIcon] = useState<string | null>(null);
  const [truncated, setTruncated] = useState(false);
  const bodyRef = useRef<HTMLParagraphElement | null>(null);

  useEffect(() => {
    getConfig()
      .then((config) => setLang(resolveLang(config.general.language)))
      .catch(() => {});

    const label = getCurrentWindow().label;
    getToastPayload().then((payload) => {
      if (!payload) {
        // 弹窗池窗口：无负载 = 热待命，保持隐藏（不销毁 webview）
        if (label !== "toast-pool") toastClose();
        return;
      }
      setPayload(payload);
    });
    // 池复用：Rust 换好负载后广播，重取并渲染（量高度 → 显示 由既有流程完成）
    const unlisten = listen("toast-payload", () => {
      getToastPayload().then((p) => {
        if (p) setPayload(p);
      });
    });
    // 通话结束：后端入库后广播，常驻的「当前通话」卡片自动关闭
    const unlistenEnd = listen("call-ended", () => { toastClose(); });
    return () => {
      unlisten.then((f) => f());
      unlistenEnd.then((f) => f());
    };
  }, []);

  // 联网获取的/手动设置的图标（Rust 缓存，data URL）优先于内置品牌图
  useEffect(() => {
    const appId = payload?.app_id;
    if (!appId) { setCachedIcon(null); return; }
    getAppIcon(appId).then((url) => setCachedIcon(url)).catch(() => setCachedIcon(null));
  }, [payload?.app_id]);

  // 渲染完后量出真实高度报给 Rust：它调窗口尺寸 + 重排 + 显示。
  useEffect(() => {
    if (!payload) return;
    const card = document.querySelector(".toast");
    if (!card) return;
    // +36 = body 上下各 18px 留白，给阴影留出空间，避免被窗口边界切断
    const height = Math.ceil(card.getBoundingClientRect().height) + 36;
    toastResize(height);
  }, [payload, error, lang]);

  // 小窗：检测正文是否被 3 行截断 → 显示「查看全文」
  useEffect(() => {
    if (expanded || !payload) {
      setTruncated(false);
      return;
    }
    const el = bodyRef.current;
    if (!el) return;
    setTruncated(el.scrollHeight > el.clientHeight + 1);
  }, [payload, expanded, lang]);

  if (!payload) {
    return null;
  }

  const t = dict(lang);
  // 电话类：category=1 或来源是系统电话（部分安卓 ANCS 兼容层用自定义类别）
  const isCall = payload.category === 1 || payload.category === 12 || /mobilephone/i.test(payload.app_id ?? "");
  // 演示勿扰询问卡：category=2（Rust 检测到全屏演示时弹出，非真实通知）
  const isDnd = payload.category === 2;
  // 更新询问卡：category=3（启动 10 秒自动检查到新版本时弹出，非真实通知）
  const isUpdateAsk = payload.category === 3;
  // 「当前通话」：category=12（接听后 iOS 推的第二条通知）→ 只留挂断、常驻不倒计时
  const isInCall = payload.category === 12 || /当前通话|通话中/.test(payload.body);
  const asset = isCall ? "/brands/phone.png" : (cachedIcon ?? brandAsset(payload.app_id));

  const onCopy = async () => {
    if (copied) return;
    const code = await toastClickForMain().catch(() => null);
    if (code) {
      setCopied(true);
      if (!expanded) setTimeout(() => toastClose(), 700);
    } else {
      toastClose();
    }
  };

  // 小窗：点卡片任意处（除按钮）→ 关闭弹窗并通知主窗口定位到该消息
  const onCardClick = async () => {
    try {
      await toastClickForMain();
      toastClose();
    } catch (err) {
      setError(String(err));
      toastClose();
    }
  };

  // 更新询问卡：蓝色系（更新语义），两按钮；30 秒无操作自动按「稍后再说」关闭
  if (isUpdateAsk) {
    const now = new Date().toLocaleTimeString(lang === "zh" ? "zh-CN" : "en-US", { hour: "2-digit", minute: "2-digit" });
    return (
      <div className="toast toast-update">
        <div className="toast-icon update-icon">
          <svg viewBox="0 0 64 64" style={{ width: "100%", height: "100%", display: "block" }}>
            <path d="M32 10 L44 26 H36 V40 H28 V26 H20 Z" fill="#ffffff" />
            <path d="M16 46 H48 V52 H16 Z" fill="#ffffff" />
          </svg>
        </div>
        <div className="toast-text">
          <div className="toast-origin">
            <span className="toast-origin-title">{payload.origin}</span>
            <span className="toast-time">{now}</span>
          </div>
          <p className="toast-body">{payload.body}</p>
          <div className="toast-dnd-actions">
            <button className="toast-later" onClick={() => toastClose()}>
              {t.ui.updateLater}
            </button>
            <button className="toast-install" onClick={() => { void updateAskInstall().catch(() => {}); toastClose(); }}>
              {t.ui.updateInstall}
            </button>
          </div>
        </div>
      </div>
    );
  }

  // 演示勿扰询问卡：复用来电卡格式（紫边框 / 紫底月牙 / 铃铛双按钮），点选择即生效
  if (isDnd) {
    const now = new Date().toLocaleTimeString(lang === "zh" ? "zh-CN" : "en-US", { hour: "2-digit", minute: "2-digit" });
    const choose = (accept: boolean) => {
      void dndChoice(accept).catch(() => {});
      toastClose();
    };
    return (
      <div className="toast toast-dnd">
        <div className="toast-icon dnd-icon">
          <svg viewBox="0 0 64 64" style={{ width: "100%", height: "100%", display: "block" }}>
            <g transform="translate(-10.56,-6) scale(0.38)" fill="#ffffff">
              <path d="M 102.81 30.06 A 70 70 0 1 0 150.29 148.70 A 64 64 0 0 1 102.81 30.06 Z" />
            </g>
          </svg>
        </div>
        <div className="toast-text">
          <div className="toast-origin">
            <span className="toast-origin-title">{payload.origin}</span>
            <span className="toast-time">{now}</span>
          </div>
          <p className="toast-body">{payload.body}</p>
          <div className="toast-dnd-actions">
            <button className="toast-keep" onClick={() => choose(false)}>
              <svg viewBox="0 0 24 24" style={{ width: 14, height: 14, verticalAlign: "-2px", marginRight: 4 }}>
                <path d="M12 22a2.2 2.2 0 0 0 2.2-2.2H9.8A2.2 2.2 0 0 0 12 22zm6.4-5.2c-.7-.6-1.4-1.5-1.4-3V9.5C17 6.5 15.2 4 12 4S7 6.5 7 9.5v4.3c0 1.5-.7 2.4-1.4 3-.3.2-.4.6-.3.9.1.4.5.6.9.6h11.6c.4 0 .8-.2.9-.6.1-.3 0-.7-.3-.9z" fill="currentColor" />
              </svg>
              {t.ui.dndKeep}
            </button>
            <button className="toast-enable" onClick={() => choose(true)}>
              <svg viewBox="0 0 24 24" style={{ width: 14, height: 14, verticalAlign: "-2px", marginRight: 4 }}>
                <path d="M12 22a2.2 2.2 0 0 0 2.2-2.2H9.8A2.2 2.2 0 0 0 12 22zm6.4-5.2c-.7-.6-1.4-1.5-1.4-3V9.5C17 6.5 15.2 4 12 4S7 6.5 7 9.5v4.3c0 1.5-.7 2.4-1.4 3-.3.2-.4.6-.3.9.1.4.5.6.9.6h11.6c.4 0 .8-.2.9-.6.1-.3 0-.7-.3-.9z" fill="currentColor" />
                <path d="M4 3L20 21" stroke="currentColor" strokeWidth={2} strokeLinecap="round" fill="none" />
              </svg>
              {t.ui.dndEnable}
            </button>
          </div>
        </div>
      </div>
    );
  }

  // 大窗：仿 macOS 信息（iMessage）阅读窗口
  if (expanded && payload) {
      return (
      <div className="toast toast-expanded imsg" onClick={onCardClick}>
        <header className="imsg-head">
          <button className="toast-dismiss imsg-close" onClick={(event) => { event.stopPropagation(); toastClose(); }} aria-label={t.toast.dismiss}>×</button>
          <div className={`toast-icon ${asset ? "brand-official" : ""}`} style={{ width: 44, height: 44, borderRadius: 12 }}>
            {asset ? <img className="toast-icon-img" src={asset} alt="" /> : (
              <svg viewBox="0 0 64 64" style={{ width: "100%", height: "100%", display: "block" }}>
                <defs>
                  <linearGradient id="zhixin-generic-app" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0" stop-color="rgb(142,154,166)" />
                    <stop offset="1" stop-color="rgb(92,107,122)" />
                  </linearGradient>
                </defs>
                <rect x="2" y="2" width="60" height="60" rx="14" fill="url(#zhixin-generic-app)" />
                <path d="M32 15c-10.5 0-19 6.7-19 15 0 4.7 2.9 8.9 7.4 11.6L18.6 49l8.1-3.3c1.8.5 3.7.7 5.3.7 10.5 0 19-6.7 19-15S42.5 15 32 15z" fill="#ffffff" />
              </svg>
            )}
          </div>
          <div className="imsg-head-text">
            <strong>{payload.origin}</strong>
            <small>{payload.open_app_name ?? ""}</small>
          </div>
          <span className="toast-time">{payload.time ?? new Date().toLocaleTimeString(lang === "zh" ? "zh-CN" : "en-US", { hour: "2-digit", minute: "2-digit" })}</span>
        </header>
        <div className="imsg-body">
          <div className="imsg-bubble">
            <p className="imsg-text">{payload.body}</p>
            <small className="imsg-time">{payload.time ?? new Date().toLocaleTimeString(lang === "zh" ? "zh-CN" : "en-US", { hour: "2-digit", minute: "2-digit" })}</small>
          </div>
        </div>
        <footer className="imsg-foot">
          {payload.code && (
            <button className={`toast-code ${copied ? "copied" : ""}`} onClick={(event) => { event.stopPropagation(); void onCopy(); }}>
              {copied ? t.toast.copied : t.toast.copyCode(payload.code)}
            </button>
          )}
          <button className="imsg-copyall" onClick={(event) => { event.stopPropagation(); void navigator.clipboard.writeText(payload.body); setCopied(true); }}>
            {t.toast.copyAll ?? "复制全文"}
          </button>
          {error && <p className="toast-error" role="alert">{error}</p>}
        </footer>
      </div>
    );
  }

  return (
    <div className={`toast ${expanded ? "toast-expanded" : ""} ${isCall ? "toast-call" : ""}`} onClick={onCardClick}>
      <div className={`toast-icon ${asset ? "brand-official" : ""}`}>
        {asset ? (
          <img className="toast-icon-img" src={asset} alt="" />
        ) : (
          <svg viewBox="0 0 64 64" style={{ width: "100%", height: "100%", display: "block" }}>
            <defs>
              <linearGradient id="zhixin-generic-app" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0" stop-color="rgb(142,154,166)" />
                <stop offset="1" stop-color="rgb(92,107,122)" />
              </linearGradient>
            </defs>
            <rect x="2" y="2" width="60" height="60" rx="14" fill="url(#zhixin-generic-app)" />
            <path
              d="M32 15c-10.5 0-19 6.7-19 15 0 4.7 2.9 8.9 7.4 11.6L18.6 49l8.1-3.3c1.8.5 3.7.7 5.3.7 10.5 0 19-6.7 19-15S42.5 15 32 15z"
              fill="#ffffff"
            />
          </svg>
        )}
      </div>
      <div className="toast-text">
        <div className="toast-origin">
          <span className="toast-origin-title">{payload.origin}</span>
          <span className="toast-time">{new Date().toLocaleTimeString(lang === "zh" ? "zh-CN" : "en-US", { hour: "2-digit", minute: "2-digit" })}</span>
        </div>
        {isCall && payload.carrier ? (
          <p className="toast-body">{payload.carrier}</p>
        ) : (
          <p ref={bodyRef} className={`toast-body ${truncated ? "truncated" : ""}`}>
            {payload.body}
            {truncated && <span className="toast-more"> 查看全文</span>}
          </p>
        )}
        {/* 当前通话：只留一个长条「挂断」（真挂断手机），常驻不倒计时 */}
        {isInCall ? (
          <div className="toast-call-actions">
            <button
              className="toast-hangup"
              onClick={async (event) => {
                event.stopPropagation();
                try {
                  await invoke("hangup_call", { uid: payload.uid });
                } catch {
                  /* 挂断失败保持弹窗，不打断 */
                  return;
                }
                toastClose();
              }}
            >
              <svg viewBox="0 0 24 24" style={{ width: 14, height: 14, verticalAlign: "-2px", marginRight: 4 }}>
                <path
                  d="M12 9c-1.6 0-3.15.25-4.6.72v3.1c0 .39-.23.74-.56.9-.98.49-1.87 1.12-2.66 1.85-.18.18-.43.28-.7.28-.28 0-.53-.11-.71-.29L.29 13.08c-.18-.17-.29-.42-.29-.7 0-.28.11-.53.29-.71C3.34 8.78 7.46 7 12 7s8.66 1.78 11.71 4.67c.18.18.29.43.29.71 0 .28-.11.53-.29.7l-2.48 2.48c-.18.18-.43.29-.71.29-.27 0-.52-.1-.7-.28-.79-.73-1.68-1.36-2.66-1.85-.33-.16-.56-.51-.56-.9v-3.1C15.15 9.25 13.6 9 12 9z"
                  fill="#ff8a80"
                />
              </svg>
              挂断
            </button>
          </div>
        ) : (
          /* 来电响铃：拒接 + 接听 */
          isCall && (
            <div className="toast-call-actions">
              <button
                className="toast-reject"
                onClick={async (event) => {
                  event.stopPropagation();
                  try {
                    await invoke("reject_call", { uid: payload.uid });
                  } catch {
                    /* 拒接失败保持弹窗，不打断 */
                  }
                  toastClose();
                }}
              >
                <svg viewBox="0 0 24 24" style={{ width: 14, height: 14, verticalAlign: "-2px", marginRight: 4 }}>
                  <path
                    d="M12 9c-1.6 0-3.15.25-4.6.72v3.1c0 .39-.23.74-.56.9-.98.49-1.87 1.12-2.66 1.85-.18.18-.43.28-.7.28-.28 0-.53-.11-.71-.29L.29 13.08c-.18-.17-.29-.42-.29-.7 0-.28.11-.53.29-.71C3.34 8.78 7.46 7 12 7s8.66 1.78 11.71 4.67c.18.18.29.43.29.71 0 .28-.11.53-.29.7l-2.48 2.48c-.18.18-.43.29-.71.29-.27 0-.52-.1-.7-.28-.79-.73-1.68-1.36-2.66-1.85-.33-.16-.56-.51-.56-.9v-3.1C15.15 9.25 13.6 9 12 9z"
                    fill="#ff8a80"
                  />
                </svg>
                拒接
              </button>
              <button
                className="toast-answer"
                onClick={async (event) => {
                  event.stopPropagation();
                  try {
                    await invoke("answer_call", { uid: payload.uid });
                  } catch {
                    /* 接听失败保持弹窗，不打断 */
                  }
                  toastClose();
                }}
              >
                <svg viewBox="0 0 24 24" style={{ width: 14, height: 14, verticalAlign: "-2px", marginRight: 4 }}>
                  <path
                    d="M6.62 10.79c1.44 2.83 3.76 5.14 6.59 6.59l2.2-2.2c.27-.27.67-.36 1.02-.24 1.12.37 2.33.57 3.57.57.55 0 1 .45 1 1V20c0 .55-.45 1-1 1-9.39 0-17-7.61-17-17 0-.55.45-1 1-1h3.5c.55 0 1 .45 1 1 0 1.25.2 2.45.57 3.57.11.35.03.74-.25 1.02l-2.2 2.2z"
                    fill="#34c759"
                  />
                </svg>
                接听
              </button>
            </div>
          )
        )}
        {payload.code && <div className="toast-actions">
          <button className={`toast-code ${copied ? "copied" : ""}`} onClick={(event) => { event.stopPropagation(); void onCopy(); }}>
            {copied ? t.toast.copied : t.toast.copyCode(payload.code)}
          </button>
        </div>}
        {error && <p className="toast-error" role="alert">{error}</p>}
      </div>
      <button
        className="toast-dismiss"
        onClick={(event) => {
          event.stopPropagation();
          toastClose();
        }}
        aria-label={t.toast.dismiss}
      >
        ×
      </button>
      {!expanded && !isInCall && (
        <div
          className="toast-countdown"
          style={{ animationDuration: `${payload.duration_secs}s` }}
        />
      )}
    </div>
  );
}
