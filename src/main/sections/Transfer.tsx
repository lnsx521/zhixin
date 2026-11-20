import { useContext, useEffect, useRef, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { renderSVG } from "uqr";
import { getTransferInfo, getMdnsHostname, listHttpAddresses, pickTransferFolder, setTransferFolder, type LocalAddress, type TransferInfo } from "../../shared/api";
import { ReceiverControls } from "./Network";
import { LangContext } from "../../shared/i18n";
import { networkDict } from "../../shared/network-i18n";
import { Icon } from "../../shared/Icon";
import { Disclosure, Row, Section } from "../components";

/** 「文件互传」页：手机浏览器扫码打开电脑上的页面，双向传文件。 */
export function Transfer() {
  const lang = useContext(LangContext);
  const nt = networkDict[lang];
  const zh = lang === "zh";
  const text = (zhText: string, enText: string) => (zh ? zhText : enText);

  const [info, setInfo] = useState<TransferInfo | null>(null);
  const [addresses, setAddresses] = useState<LocalAddress[]>([]);
  const [ip, setIp] = useState("");
  const [domain, setDomain] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy] = useState(false);
  const [copied, setCopied] = useState(false);
  const copyTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(copyTimer.current), []);

  useEffect(() => {
    let disposed = false;
    void getTransferInfo().then((i) => { if (!disposed) setInfo(i); }).catch((e) => { if (!disposed) setError(String(e)); });
    void getMdnsHostname().then((h) => { if (!disposed) setDomain(h); }).catch(() => {});
    void listHttpAddresses().then((a) => {
      if (disposed) return;
      setAddresses(a);
      setIp(a.find((x) => !x.address.startsWith("127.") && !x.address.startsWith("169.254."))?.address ?? "");
    }).catch(() => {});
    return () => { disposed = true; };
  }, []);

  const refresh = async () => {
    try { setInfo(await getTransferInfo()); } catch (e) { setError(String(e)); }
  };

  const copy = async (value: string) => {
    try {
      await writeText(value);
      clearTimeout(copyTimer.current);
      setCopied(true);
      copyTimer.current = setTimeout(() => setCopied(false), 2000);
    } catch { setError(text("复制失败", "Copy failed")); }
  };


  const urls = info?.enabled && info.token
    ? addresses
        .filter((a) => !a.address.startsWith("169.254."))
        .map((a) => ({ label: a.address, name: a.name, url: `http://${a.address}:${info.port}/transfer?t=${encodeURIComponent(info.token ?? "")}` }))
    : [];
  const domainUrl = info?.enabled && info.token && domain
    ? `http://${domain}:${info.port}/transfer?t=${encodeURIComponent(info.token)}`
    : null;
  const qr = domainUrl ?? urls.find((u) => u.label === ip)?.url ?? urls[0]?.url;

  if (info && !info.enabled) {
    return (
      <>
        <div className="hero-card hero-off">
          <div className="hero-icon"><Icon name="folder" size={28} /></div>
          <div className="hero-body">
            <h2>{text("先开启网络接入", "Enable the network receiver first")}</h2>
            <p>{text("文件互传与短信接收共用同一个端口与令牌。到「网络接入」页打开接收器，回到这里就能看到二维码。", "File transfer shares the port and token with SMS receiving. Turn on the receiver on the Network page, then come back for the QR code.")}</p>
          </div>
        </div>
      </>
    );
  }

  return (
    <>
      <div className="hero-card">
        {qr && <div className="hero-qr" aria-label="QR code" dangerouslySetInnerHTML={{ __html: renderSVG(qr, { border: 2 }) }} />}
        <div className="hero-body">
          {domain && (
            <span
              className="hero-badge"
              style={{ cursor: "pointer" }}
              title={text("点击复制域名", "Click to copy domain")}
              onClick={() => { void copy(domain ?? ""); }}
            >
              {domain}
            </span>
          )}
          <h2>{text("手机扫码，双向传文件", "Scan to transfer files both ways")}</h2>
          <p>{text("手机与电脑连同一 Wi-Fi，Safari 扫码打开页面：选文件上传、拍照上传，也能把电脑上的文件下载到手机。固定域名换网络不用重新扫码。", "With both devices on the same Wi-Fi, open the page in Safari: pick files or shoot photos to upload, and download files from this PC. The fixed domain survives network changes.")}</p>
          <div className="hero-actions">
            {domainUrl && <button className="btn primary" onClick={() => {
              void copy(domainUrl);
              window.setTimeout(() => window.alert(text(
                "令牌已复制。请勿将令牌分享给他人——拿到这条地址即可上传、下载和删除电脑文件。",
                "Token copied. Do NOT share it - anyone with this link can upload, download and delete files on this PC."
              )), 150);
            }}>{copied ? text("✓ 已复制", "✓ Copied") : text("复制令牌", "Copy token")}</button>}
          </div>
        </div>
      </div>

      <ReceiverControls />

      <Section narrow title={text("接收文件夹", "Receive folder")}>
        <Row label={info?.folder ?? "…"} hint={text("手机上传的文件保存到这里；手机页面也列出此文件夹内的文件供下载。", "Files uploaded from the phone land here; the phone page also lists files in this folder for download.")}>
          <button className="network-quiet-action" disabled={!info || busy} onClick={async () => {
            try {
              const picked = await pickTransferFolder();
              if (!picked) return;
              await setTransferFolder(picked);
              await refresh();
            } catch (err) { setError(String(err)); }
          }}>
            {text("更改文件夹", "Change folder")}
          </button>
        </Row>
      </Section>

      <Disclosure title={text("连接指南", "Connection guide")} hint={text("电脑分享给手机 + 手机发送到电脑", "PC to phone + phone to PC")}>
        <Disclosure title={text("① 电脑分享给手机（扫码访问电脑页面）", "PC to phone (scan QR)")}>
          <p className="hint-text">{text("手机扫上方二维码（或浏览器输入局域网地址）打开电脑互传页面，即可浏览和下载电脑收到的文件。", "Scan the QR above (or enter the LAN address) on the phone to open the transfer page and browse received files.")}</p>
        </Disclosure>
          <Disclosure title={nt.ios + text("（手机发送到电脑 · 文件互传）", " (phone to PC, files)")}>
          <p style={{ fontSize: 13.5, lineHeight: 1.75, margin: "0 0 10px", color: "var(--text-secondary)" }}>
            <p style={{ margin: "0 0 8px", fontWeight: 600, color: "var(--text)" }}>{text("需要两个快捷指令（各管一类内容）：", "Two shortcuts are needed:")}</p>
            <div style={{ margin: "0 0 6px" }}>
            <b>{text("快捷指令一「发送到电脑」（管照片/文件/PDF）：", "Shortcut 1 - files:")}</b>
            <ol style={{ margin: "6px 0 0", paddingLeft: 22, fontSize: 13, lineHeight: 1.9, color: "var(--text-secondary)" }}>
              <li>{text("快捷指令 App 新建，添加「从共享表单接收」，类型勾选「图像」「文件」「PDF 文本」；", "Create a new shortcut, add Receive from Share Sheet with Images / Files / PDF enabled;")}</li>
              <li>{text("添加「获取 URL 内容」：方法 POST，URL 填上方局域网地址，请求体选「表单」，新字段类型选「文件」，Key 输入 file（小写），值选「快捷指令输入」；", "Add Get Contents of URL: POST to the LAN address above, Form body, File-type field with key file (lowercase), value Shortcut Input;")}</li>
              <li>{text("命名「发送到电脑」保存。", "Name it and save.")}</li>
            </ol>
          </div>
            <div style={{ margin: "0 0 6px" }}>
            <b>{text("快捷指令二「发送链接到电脑」（管网页/链接/文本）：", "Shortcut 2 - links:")}</b>
            <ol style={{ margin: "6px 0 0", paddingLeft: 22, fontSize: 13, lineHeight: 1.9, color: "var(--text-secondary)" }}>
              <li>{text("复制快捷指令一；", "Duplicate shortcut 1;")}</li>
              <li>{text("接收类型取消「图像」「文件」，只勾「Safari 浏览器」「网页 URL」「文章」「链接」；", "Uncheck Images / Files, check Safari Web Page / URLs / Articles / Links;")}</li>
              <li>{text("表单字段删除 file 那行，重新添加新字段：类型「文本」，Key 输入 text（小写），值选「快捷指令输入」；", "Delete the file field, add a Text-type field with key text (lowercase), value Shortcut Input;")}</li>
              <li>{text("改名「发送链接到电脑」保存。", "Rename and save.")}</li>
            </ol>
          </div>
            <p style={{ margin: 0 }}>{text("之后在相册/文件里分享用快捷指令一，在 Safari 分享网页时用快捷指令二。电脑会自动抓网页标题生成快捷方式文件。", "Share files with #1; share web pages with #2. The PC fetches the page title and creates a .url shortcut file.")}</p>
            <div style={{ margin: "12px 0 0", padding: "10px 12px", borderLeft: "3px solid #2f6feb", background: "#eef4fe", borderRadius: "0 8px 8px 0" }}>
              <b style={{ fontSize: 12.5, color: "#1f2733" }}>提示</b>
              <span style={{ fontSize: 12.5, color: "#1f2733" }}>：如果您熟悉 iOS 快捷指令，可将「获取 URL 内容」动作合并到现有的自动化中统一管理——多条通知转发可共用一条自动化，效果与分步创建相同。</span>
            </div>
          </p>
        </Disclosure>
        <Disclosure title={nt.android + text("（第三方推送 · 进阶）", " (third-party push, advanced)")}>
          <p style={{ margin: "0 0 8px" }}>{text("说明：本软件的蓝牙通道基于苹果私有协议（ANCS），仅 iPhone 可自动接收通知；安卓系统不支持该协议，蓝牙无法推送。", "The Bluetooth channel uses Apple's proprietary ANCS protocol - iPhone only. Android cannot push over Bluetooth.")}</p>
          <p style={{ margin: "0 0 8px" }}>{text("安卓用户的替代方案：在安卓手机上安装任意支持自定义 HTTP 请求的推送工具（如 HTTP Shortcuts、MacroDroid），按以下要点配置即可把消息推送到电脑弹窗：", "Android alternative: install any HTTP-capable automation tool (HTTP Shortcuts, MacroDroid) and configure as follows:")}</p>
          <ol style={{ margin: "0 0 8px", paddingLeft: 22, fontSize: 13, lineHeight: 1.8, color: "var(--text-secondary)" }}>
            <li>{text("手机与电脑连同一个局域网；", "Phone and PC on the same LAN;")}</li>
            <li>{text("请求地址填上方「接收链接」，方法选 GET 或 POST；", "Use the receiver link above, GET or POST;")}</li>
            <li>{text("携带接入令牌（token 参数或 Authorization 头）；", "Include the token (query param or Authorization header);")}</li>
            <li>{text("正文填入 body 参数（URL 编码）。", "Put the message text in the body parameter (URL-encoded).")}</li>
          </ol>
          <p className="hint-text">{text("本软件并非专为安卓开发，以上为通用协议的自助用法，效果与 iPhone 蓝牙推送相同（仅弹窗提示）。", "Not built for Android, but the generic protocol works the same - popup notifications only.")}</p>
        </Disclosure>
      </Disclosure>


      <Section narrow title={text("安全提醒", "Security notice")} description={<>{text("令牌是访问电脑的唯一凭证，泄露后请到", "The token is the sole credential - if leaked, reset it under ")}<span className="path-ref">{text("「接收服务 → 高级设置」", "Receiver service - Advanced settings")}</span>{text("中重置。", ".")}</>} />

      {error && <p className="bad-text" role="alert">{error}</p>}
    </>
  );
}
