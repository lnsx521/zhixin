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

        </Row>
      </Section>

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

      <Disclosure title={text("手机连接指南", "Phone connection guide")} hint={text("电脑分享给手机 + 手机发送到电脑", "PC to phone + phone to PC")}>
        <Disclosure title={text("① 电脑分享给手机（扫码访问电脑页面）", "PC to phone (scan QR)")}>
          <p className="hint-text">{text("手机扫上方二维码（或浏览器输入局域网地址）打开电脑互传页面，即可浏览和下载电脑收到的文件。", "Scan the QR above (or enter the LAN address) on the phone to open the transfer page and browse received files.")}</p>
        </Disclosure>
          <Disclosure title={nt.ios + text("（手机发送到电脑 · 文件互传）", " (phone to PC, files)")}>
          <p style={{ fontSize: 13.5, lineHeight: 1.75, margin: "0 0 10px", color: "var(--text-secondary)" }}>
            <p style={{ margin: "0 0 8px", fontWeight: 600, color: "var(--text)" }}>{text("需要两个快捷指令（各管一类内容）：", "Two shortcuts are needed:")}</p>
            <p style={{ margin: "0 0 6px" }}><b>{text("快捷指令一「发送到电脑」（管照片/文件/PDF）：", "Shortcut 1 - files:")}</b>{text("在快捷指令 App 新建：① 添加「从共享表单接收」，类型勾选「图像」「文件」「PDF 文本」；② 添加「获取 URL 内容」：方法 POST，URL 填上方局域网地址，请求体选「表单」，添加新字段时类型选「文件」，Key 手动输入 file（小写），值选「快捷指令输入」；③ 命名保存。", "1) Receive: Images, Files, PDF; 2) Get Contents of URL: POST to LAN address above, Form body, add a File-type field with key file and value Shortcut Input; 3) Save.")}</p>
            <p style={{ margin: "0 0 6px" }}><b>{text("快捷指令二「发送链接到电脑」（管网页/链接/文本）：", "Shortcut 2 - links:")}</b>{text("复制快捷指令一，改三处：① 接收类型取消图像/文件，只勾「Safari 浏览器」「网页 URL」「文章」「链接」；② 表单字段删除 file 那行，重新添加新字段时类型选「文本」，Key 手动输入 text，值选「快捷指令输入」；③ 改名「发送链接到电脑」保存。", "Duplicate shortcut 1: uncheck Images/Files, check Safari web page / URLs / Articles / Links; replace the file field with a Text-type field, key text, value Shortcut Input; rename it.")}</p>
            <p style={{ margin: 0 }}>{text("之后在相册/文件里分享用快捷指令一，在 Safari 分享网页时用快捷指令二。电脑会自动抓网页标题生成快捷方式文件。", "Share files with #1; share web pages with #2. The PC fetches the page title and creates a .url shortcut file.")}</p>
            <div style={{ margin: "12px 0 0", padding: "10px 12px", borderLeft: "3px solid #2f6feb", background: "#eef4fe", borderRadius: "0 8px 8px 0" }}>
              <b style={{ fontSize: 12.5, color: "#1f2733" }}>提示</b>
              <span style={{ fontSize: 12.5, color: "#1f2733" }}>：如果您熟悉 iOS 快捷指令，可将「获取 URL 内容」动作合并到现有的自动化中统一管理——多条通知转发可共用一条自动化，效果与分步创建相同。</span>
            </div>
          </p>
        </Disclosure>
        <Disclosure title={nt.android + text("（第三方推送 · 进阶）", " (third-party push, advanced)")}>
          <p className="hint-text">{nt.genericHint}</p>
          <p className="hint-text">{text("本软件并非专为安卓开发，熟悉安卓的用户可用此协议自行推送。", "Not built for Android, but Android users may push via this protocol.")}</p>
        </Disclosure>
      </Disclosure>


      <Section narrow title={text("安全提醒", "Security notice")} description={<>{text("令牌是访问电脑的唯一凭证，泄露后请到", "The token is the sole credential - if leaked, reset it under ")}<span className="path-ref">{text("「接收服务 → 高级设置」", "Receiver service - Advanced settings")}</span>{text("中重置。", ".")}</>} />

      {error && <p className="bad-text" role="alert">{error}</p>}
    </>
  );
}
