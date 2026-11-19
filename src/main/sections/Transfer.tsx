import { useContext, useEffect, useRef, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { renderSVG } from "uqr";
import { getTransferInfo, getMdnsHostname, listHttpAddresses, setTransferFolder, type LocalAddress, type TransferInfo } from "../../shared/api";
import { LangContext } from "../../shared/i18n";
import { Icon } from "../../shared/Icon";
import { Disclosure, Row, Section } from "../components";

/** 「文件互传」页：手机浏览器扫码打开电脑上的页面，双向传文件。 */
export function Transfer() {
  const lang = useContext(LangContext);
  const zh = lang === "zh";
  const text = (zhText: string, enText: string) => (zh ? zhText : enText);

  const [info, setInfo] = useState<TransferInfo | null>(null);
  const [addresses, setAddresses] = useState<LocalAddress[]>([]);
  const [ip, setIp] = useState("");
  const [domain, setDomain] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
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

  const applyFolder = async () => {
    if (busy || !draft.trim()) return;
    setBusy(true); setError(null);
    try {
      await setTransferFolder(draft);
      setEditing(false);
      await refresh();
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
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
        {error && <p className="bad-text" role="alert">{error}</p>}
      </>
    );
  }

  return (
    <>
      <div className="hero-card">
        {qr && <div className="hero-qr" aria-label="QR code" dangerouslySetInnerHTML={{ __html: renderSVG(qr, { border: 2 }) }} />}
        <div className="hero-body">
          {domain && <span className="hero-badge">{domain}</span>}
          <h2>{text("手机扫码，双向传文件", "Scan to transfer files both ways")}</h2>
          <p>{text("手机与电脑连同一 Wi-Fi，Safari 扫码打开页面：选文件上传、拍照上传，也能把电脑上的文件下载到手机。固定域名换网络不用重新扫码。", "With both devices on the same Wi-Fi, open the page in Safari: pick files or shoot photos to upload, and download files from this PC. The fixed domain survives network changes.")}</p>
          <div className="hero-actions">
            {domainUrl && <button className="btn primary" onClick={() => void copy(domainUrl)}>{copied ? text("✓ 已复制", "✓ Copied") : text("复制手机访问地址", "Copy phone link")}</button>}
          </div>
        </div>
      </div>

      <Section narrow title={text("接收文件夹", "Receive folder")}>
        <Row label={info?.folder ?? "…"} hint={text("手机上传的文件保存到这里；手机页面也列出此文件夹内的文件供下载。", "Files uploaded from the phone land here; the phone page also lists files in this folder for download.")}>
          {editing ? (
            <div className="transfer-edit">
              <input
                className="input"
                value={draft}
                placeholder={info?.folder ?? "D:\\Files"}
                disabled={busy}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") void applyFolder(); }}
              />
              <button className="btn" disabled={busy || !draft.trim()} onClick={() => void applyFolder()}>{text("应用", "Apply")}</button>
              <button className="network-quiet-action" disabled={busy} onClick={() => { setEditing(false); setError(null); }}>{text("取消", "Cancel")}</button>
            </div>
          ) : (
            <button className="network-quiet-action" disabled={!info || busy} onClick={() => { setEditing(true); setDraft(info?.folder ?? ""); }}>
              {text("更改文件夹", "Change folder")}
            </button>
          )}
        </Row>
      </Section>

      {urls.length > 0 && (
        <Section narrow title={text("局域网地址（备用）", "LAN addresses (fallback)")} description={text("域名不可用时的直连地址。", "Direct addresses for when the domain is unreachable.")}>
          <div className="network-address-list">
            {urls.map((u) => (
              <Row key={u.label} label={u.label} hint={u.name}>
                <button className="network-quiet-action" onClick={() => void copy(u.url)}>{copied ? text("已复制", "Copied") : text("复制", "Copy")}</button>
              </Row>
            ))}
          </div>
        </Section>
      )}
      {info?.enabled && urls.length === 0 && (
        <p className="hint-text">{text("未找到可用网卡地址。", "No usable network address found.")}</p>
      )}

      <Disclosure title={text("iOS 快捷指令设置（把手机文件分享到电脑）", "iOS Shortcuts setup (share phone files to PC)")} hint={text("教程已从手机页移到这里", "Moved from the phone page")}>
        <p style={{ fontSize: 13.5, lineHeight: 1.75, margin: "0 0 10px", color: "var(--text-secondary)" }}>
          <p style={{ margin: "0 0 8px", fontWeight: 600, color: "var(--text)" }}>{text("在 iPhone 的快捷指令 App 新建指令：", "Create a shortcut in the iPhone Shortcuts app:")}</p>
          <p style={{ margin: "0 0 6px" }}>{text("① 添加「从共享表单接收」，类型勾选「图像」「文件」和「文本」；", "1) Add 'Receive from Share Sheet', check Images, Files and Text;")}</p>
          <p style={{ margin: "0 0 6px" }}>{text("② 添加「获取 URL 内容」：方法选 POST，URL 填上方局域网地址，请求体选「表单」，添加字段 file，值选「快捷指令输入」；", "2) Add 'Get contents of URL' — POST to the LAN address above, Form body, field file = Shortcut Input;")}</p>
          <p style={{ margin: 0 }}>{text("③ 命名保存。以后任何内容点「分享」→ 选它，直接传到这台电脑。", "3) Save. Share anything to it and it lands on this PC.")}</p>
        </p>
      </Disclosure>

      {error && <p className="bad-text" role="alert">{error}</p>}
    </>
  );
}
