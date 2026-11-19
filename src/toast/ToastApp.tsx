import { useEffect, useRef, useState } from "react";
import {
  getConfig,
  getAppIcon,
  getToastPayload,
  toastClick,
  toastClose,
  toastExpand,
  toastResize,
  toastOpenApp,
  type ToastPayload,
} from "../shared/api";
import { dict, resolveLang, type Lang } from "../shared/i18n";
import { Icon } from "../shared/Icon";
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
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lang, setLang] = useState<Lang>("zh");
  const [cachedIcon, setCachedIcon] = useState<string | null>(null);
  const [truncated, setTruncated] = useState(false);
  const bodyRef = useRef<HTMLParagraphElement | null>(null);

  useEffect(() => {
    getConfig()
      .then((config) => setLang(resolveLang(config.general.language)))
      .catch(() => {});

    getToastPayload().then((payload) => {
      if (!payload) {
        // 没有负载（比如兜底定时器已把它清掉）→ 直接关
        toastClose();
        return;
      }
      setPayload(payload);
    });
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
    const height = Math.ceil(card.getBoundingClientRect().height) + 8; // body 上下各 4px 留白
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
  const isCall = payload.category === 1;
  const asset = isCall ? "/brands/phone.png" : (cachedIcon ?? brandAsset(payload.app_id));

  const onCopy = async () => {
    if (copied) return;
    const code = await toastClick().catch(() => null);
    if (code) {
      setCopied(true);
      if (!expanded) setTimeout(() => toastClose(), 700);
    } else {
      toastClose();
    }
  };

  const onOpen = async () => {
    if (!payload.open_app_name) { await onCopy(); return; }
    if (opening) return;
    setOpening(true); setError(null);
    try { await toastOpenApp(); await toastClose(); }
    catch (err) { setError(String(err)); setOpening(false); }
  };

  // 小窗：点卡片任意处（除按钮）→ 展开成大窗；大窗点空白不动作
  const onCardClick = () => {
    if (!expanded) void toastExpand().catch(() => {});
  };

  return (
    <div className={`toast ${expanded ? "toast-expanded" : ""} ${isCall ? "toast-call" : ""}`} onClick={onCardClick}>
      <div className={`toast-icon ${asset ? "brand-official" : ""}`}>
        {asset ? (
          <img className="toast-icon-img" src={asset} alt="" />
        ) : (
          <Icon name={payload.code ? "code" : "message"} size={20} />
        )}
      </div>
      <div className="toast-text">
        <div className="toast-origin">
          <span className="toast-origin-title">{payload.origin}</span>
          <span className="toast-time">{new Date().toLocaleTimeString(lang === "zh" ? "zh-CN" : "en-US", { hour: "2-digit", minute: "2-digit" })}</span>
        </div>
        <p ref={bodyRef} className={`toast-body ${truncated ? "truncated" : ""}`}>
          {payload.body}
          {truncated && <span className="toast-more"> 查看全文</span>}
        </p>
        {(payload.open_app_name || payload.code) && <div className="toast-actions">
          {payload.code && (
            <button className={`toast-code ${copied ? "copied" : ""}`} onClick={(event) => { event.stopPropagation(); void onCopy(); }}>
              {copied ? t.toast.copied : t.toast.copyCode(payload.code)}
            </button>
          )}
          {payload.open_app_name && <button className="toast-open" disabled={opening} onClick={(event) => { event.stopPropagation(); void onOpen(); }}>{opening ? t.rules.opening : t.rules.open(payload.open_app_name)}</button>}
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
      {!expanded && (
        <div
          className="toast-countdown"
          style={{ animationDuration: `${payload.duration_secs}s` }}
        />
      )}
    </div>
  );
}
