import { useContext, useEffect, useRef, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { configureHttp, getHttpStatus, getMdnsHostname, listHttpAddresses, resetHttpToken, setHttpCopy, type HttpStatus, type LocalAddress } from "../../shared/api";
import { LangContext } from "../../shared/i18n";
import { networkDict } from "../../shared/network-i18n";
import { Modal, NumberField, Row, Section, Toggle } from "../components";

export function ReceiverControls() {
  const lang=useContext(LangContext), t=networkDict[lang];
  const text=(zh:string,en:string)=>lang==="zh"?zh:en;
  const [status,setStatus]=useState<HttpStatus|null>(null),[bind,setBind]=useState("0.0.0.0"),[port,setPort]=useState(24836),[advOpen,setAdvOpen]=useState(false);
  useEffect(()=>{void listHttpAddresses().then(a=>{if(a.length)setAddresses(a);}).catch(()=>{});},[]);
  const [addresses,setAddresses]=useState<LocalAddress[]>([]),[ip,setIp]=useState(""),[dialog,setDialog]=useState<"ip"|"reset"|null>(null);
  const [busy,setBusy]=useState(false),[error,setError]=useState<string|null>(null),[copied,setCopied]=useState<string|null>(null);
  const [domain,setDomain]=useState("");
  const [hostChoice,setHostChoice]=useState("");
  const copyTimer=useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(()=>()=>clearTimeout(copyTimer.current),[]);
  useEffect(()=>{let disposed=false;void getHttpStatus().then(s=>{if(!disposed){setStatus(s);setBind(s.settings.bind);setPort(s.settings.port);}}).catch(e=>{if(!disposed)setError(String(e));});void getMdnsHostname().then(h=>{if(!disposed)setDomain(h??"");}).catch(()=>{});void listHttpAddresses().then(a=>{if(!disposed){setAddresses(a);setIp(a.find(x=>!x.address.startsWith("127.")&&!x.address.startsWith("169.254."))?.address??"");}}).catch(()=>{});return()=>{disposed=true;};},[]);
  const run=async(action:()=>Promise<void>)=>{if(busy)return;setBusy(true);setError(null);try{await action();}catch(e){setError(String(e));}finally{try{setStatus(await getHttpStatus());}catch(e){setError(String(e));}setBusy(false);}};
  const host=status?.settings.bind==="127.0.0.1"?"127.0.0.1":hostChoice||domain||ip||"<PC-LAN-IP>";
  const endpoint=`http://${host}:${status?.settings.port??port}/api/v1/notifications`;
  const url=`${endpoint}?token=${encodeURIComponent(status?.token??"")}`;
  const testName=text("设备名称","Device name");
  const testBody=text("通知正文","Notification body");
  const testSender=text("发信人","Sender");
  const testUrl=`${endpoint}?${new URLSearchParams({device_name:testName,body:testBody,from:testSender,token:status?.token??""})}`;
  const displayedParams=[["device_name",testName],["body",testBody],["from",testSender],["token",status?.token??"…"]];
  const copy=async(value:string,key:string)=>{try{await writeText(value);clearTimeout(copyTimer.current);setCopied(key);copyTimer.current=setTimeout(()=>setCopied(null),2000);}catch{setError(t.clipboardError);}};
  const dirty=status&&(bind!==status.settings.bind||port!==status.settings.port);
  return <>
    <Section wide title={<>{t.receiver} <span className={"receiver-status " + (status?.running ? "on" : "off")} role="status">{status?.running ? "● 运行中" : "○ 已关闭"}</span></>} action={<><button className="network-quiet-action" onClick={()=>setAdvOpen(o=>!o)}>{text("高级设置","Advanced settings")} {advOpen?"▾":"▸"}</button><Toggle checked={status?.settings.enabled??false} disabled={busy||!status} onChange={v=>void run(()=>configureHttp(v,status!.settings.bind,status!.settings.port))}/></>}>
      {advOpen && <>
        <Row label={t.scope}><select className="select" value={bind} disabled={busy} onChange={e=>setBind(e.target.value)}><option value="127.0.0.1">{t.local}</option><option value="0.0.0.0">{t.lan}</option></select></Row>
        <Row label={t.port}><NumberField value={port} onChange={setPort} min={1} max={65535} disabled={busy}/></Row>
        <Row label={text("允许网络消息自动复制验证码","Allow automatic code copying")} hint={t.copyHint}><Toggle checked={status?.settings.allow_copy??false} disabled={busy||!status} onChange={v=>void run(()=>setHttpCopy(v))}/></Row>
        {dirty&&<button className="btn" disabled={busy||!Number.isInteger(port)||port<1||port>65535} onClick={()=>void run(()=>configureHttp(status!.settings.enabled,bind,port))}>{status?.settings.enabled?t.apply:t.save}</button>}
        <div style={{ margin: "10px 0 0", padding: "12px", border: "1px solid var(--border)", borderRadius: 10, width: "100%", alignSelf: "stretch" }} data-receiver-link><div style={{ fontSize: 13, fontWeight: 700, marginBottom: 8, display: "flex", alignItems: "center", justifyContent: "space-between", gap: 10 }}><span>接收链接（快捷指令地址）</span><button className="network-interface" disabled={!status||status.settings.bind==="127.0.0.1"} aria-haspopup="dialog" onClick={()=>{setDialog("ip");void listHttpAddresses().then(setAddresses).catch(e=>setError(String(e)));}}><span>{text("网卡","Interface")}</span><span>{host}</span><span aria-hidden="true">▾</span></button></div>
      <code className="network-receiver-url"><span className="network-url-endpoint">{endpoint}</span>{displayedParams.map(([key,value],index)=><span key={key}><span className="network-url-punctuation">{index===0?"?":"&"}</span><span className="network-url-key">{key}</span><span className="network-url-punctuation">=</span><span className={key==="token"?"network-url-token":"network-url-value"}>{value}</span></span>)}</code>
      <div className="network-link-actions">
        <button className="btn primary" disabled={!status?.token||host.includes("<")} onClick={()=>void copy(testUrl,"test")}>{copied==="test"?t.copied:text("复制链接","Copy link")}</button>
        <div className="network-link-secondary">
          <button className="network-quiet-action" disabled={!status?.token||host.includes("<")} onClick={()=>void copy(url,"sender")}>{copied==="sender"?t.copied:text("仅复制接口地址","Copy API URL")}</button>
          <button className="network-quiet-action" disabled={busy||!status} onClick={()=>setDialog("reset")} style={{ color: "#dc2626", fontWeight: 600 }}>{t.reset}</button>
        </div>
          <p className="hint-text" style={{ margin: "10px 0 0" }}>本软件暂不支持安卓。如需安卓推送效果（如蓝牙通知），欢迎基于本软件的架构与代码自行实现——本项目开源，随意改造。</p>
        <div style={{ margin: "10px 0 0", padding: "12px", border: "1px solid var(--border)", borderRadius: 10, width: "100%", alignSelf: "stretch" }}><div style={{ fontSize: 13, fontWeight: 700, marginBottom: 8 }}>局域网地址（备用）</div><p className="hint-text" style={{ margin: "0 0 6px" }}>域名不可用时的直连地址。</p>
          <div className="network-address-list">
            {addresses.map((a) => (
              <Row key={`${a.name}:${a.address}`} label={a.address} hint={a.name}>
                <button className="network-quiet-action" onClick={()=>void copy(`http://${a.address}:24836`,"lan")}>{copied==="lan"?text("已复制","Copied"):text("复制","Copy")}</button>
              </Row>
            ))}
            {addresses.length===0 && <p className="hint-text">未找到可用网卡地址。</p>}
          </div>
        </div>
      </div>
    </div>
      </>}
    </Section>
    {(error||status?.error)&&<p className="bad-text" role="alert">{error||status?.error}</p>}
    {dialog&&<Modal title={dialog==="ip"?t.chooseAddress:t.resetTitle} onClose={()=>{if(!busy)setDialog(null);}}>
      {dialog==="ip"?<><p>{t.addressHint}</p><div className="network-addresses">{domain&&<button className={`network-address${host===domain?" selected":""}`} onClick={()=>{setHostChoice(domain);setCopied(null);setDialog(null);}}><strong>{domain}</strong><small>{text("固定域名，换 Wi-Fi 不用改地址","Fixed domain, survives Wi-Fi changes")}</small></button>}{addresses.map(a=><button className={`network-address${host===a.address?" selected":""}`} key={`${a.name}:${a.address}`} onClick={()=>{setHostChoice(a.address);setCopied(null);setDialog(null);}}><strong>{a.address}</strong><small>{a.name}</small></button>)}</div>{!addresses.length&&!domain&&<p>{t.noAddresses}</p>}</>:<><p>{text("重置后所有发送端的旧链接立即失效，需要重新配置。","Resetting immediately invalidates every sender's old URL. Reconfigure all senders.")}</p><button className="btn network-danger" disabled={busy} onClick={()=>void run(async()=>{await resetHttpToken();setCopied(null);setDialog(null);})}>{t.reset}</button></>}
    </Modal>}
  </>;
}
