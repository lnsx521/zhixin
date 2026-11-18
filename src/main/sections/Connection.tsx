import { useEffect, useState } from "react";
import {
  forgetDevice,
  getLinkState,
  listDevices,
  onDevicesChanged,
  onLinkState,
  setDeviceEnabled,
  setPreferredDevice,
  type LinkStatePayload,
  type ManagedDevice,
} from "../../shared/api";
import { useT } from "../../shared/i18n";
import { Section } from "../components";
import { Icon } from "../../shared/Icon";
import { ConnectionGuide } from "./ConnectionGuide";

/** 「连接」页：链路状态 + 新手引导。 */
export function Connection() {
  const t = useT();
  const [link, setLink] = useState<LinkStatePayload | null>(null);
  const [devices, setDevices] = useState<ManagedDevice[]>([]);
  const [deviceError, setDeviceError] = useState<string | null>(null);
  const [showGuide, setShowGuide] = useState(false);
  // 配对修复引导：已配对但通知共享未开启（长时间不就绪）→ 显示修复指引
  const [notReadySince, setNotReadySince] = useState<number | null>(null);
  const [, tick] = useState(0);
  // 静默窗口：来电挂断等场景链路 ~3 秒内自愈，界面保持原状态不闪烁；
  // 超过 3 秒仍未恢复才显示「重连中」。
  const [displayLink, setDisplayLink] = useState<LinkStatePayload | null>(null);

  useEffect(() => {
    if (!link || link.ready || link.state === "adapter_unsupported" || link.state === "faulted") {
      setDisplayLink(link);
      return;
    }
    // 非就绪状态：延迟 3 秒才上屏
    const timer = setTimeout(() => setDisplayLink(link), 3000);
    return () => clearTimeout(timer);
  }, [link?.state, link?.ready, link?.label]);

  useEffect(() => {
    getLinkState().then(setLink).catch(() => {});
    listDevices().then(setDevices).catch(() => {});
    const unlisten = onLinkState(setLink);
    const unlistenDevices = onDevicesChanged(setDevices);
    const timer = setInterval(() => tick((n) => n + 1), 3000);
    return () => {
      unlisten.then((fn) => fn());
      unlistenDevices.then((fn) => fn());
      clearInterval(timer);
    };
  }, []);


  const runDeviceAction = async (action: () => Promise<void>) => {
    setDeviceError(null);
    try {
      await action();
    } catch (error) {
      setDeviceError(String(error));
    }
  };

  const unsupported = link?.state === "adapter_unsupported";
  const failed = unsupported || link?.state === "faulted";
  const ready = link?.ready ?? false;

  // 已连接但长时间不就绪 → 很可能是 iPhone 通知共享开关没开
  useEffect(() => {
    if (link && !link.ready && !unsupported && !failed) {
      if (notReadySince === null) setNotReadySince(Date.now());
    } else {
      setNotReadySince(null);
    }
  }, [link?.state, link?.ready, notReadySince, unsupported, failed]);
  const showPairingGuide =
    notReadySince !== null && Date.now() - notReadySince > 20000 && !showGuide;
  const stateLabel = link
    ? link.ready
      ? t.connection.readyTitle
      : (t.link[link.state as keyof typeof t.link] ?? link.label)
    : "…";

  return (
    <>
      <div className={`status-card ${ready ? "ok" : ""} ${failed ? "bad" : ""}`} role="status">
          <span className="status-dot" />
          <div>
            <strong>{stateLabel}</strong>
            <p className="status-detail">{failed && displayLink?.detail ? displayLink.detail : ready ? t.connection.readyHint : displayLink?.awaiting_verification ? t.connection.verificationHint : t.connection.waitingHint}</p>
          </div>
      </div>

      <Section title={t.connection.devicesTitle} description={t.connection.devicesDesc} action={devices.length > 0 ? <button className="btn" onClick={() => setShowGuide(true)}><Icon name="plus" size={15} />{t.connection.addDevice}</button> : undefined}>
        {deviceError && <p className="bad-text">{deviceError}</p>}
        {devices.length === 0 ? (
          <div className="empty-device">
            <span className="device-symbol"><Icon name="phone" size={28} /></span>
            <strong>{t.connection.emptyTitle}</strong>
            <p>{t.connection.emptyDesc}</p>
            <button className="btn primary" onClick={() => setShowGuide(true)}>{t.connection.addDevice}</button>
          </div>
        ) : (
          <div className="device-list">
            {devices.map((device) => (
              <article className={`device-card ${device.current ? "current" : ""}`} key={device.id}>
                <div className="device-main">
                   <div className="device-identity">
                     <span className="device-symbol"><Icon name="phone" size={24} /></span>
                     <div className="device-name-col">
                       <div className="device-title-row">
                         <strong>{device.name}</strong>
                         <span className="device-id">{device.address_hint}</span>
                         {device.battery_level !== null && <Battery level={device.battery_level} />}
                         {device.battery_level !== null && <span className="device-battery-pct">{device.battery_level}%</span>}
                       </div>
                       <p className="device-state">
                         {device.current
                           ? device.connected
                              ? ready ? t.connection.deviceConnected : t.connection.verificationHint
                             : t.connection.deviceConnecting
                           : device.online
                             ? t.connection.deviceOnline
                             : t.connection.deviceOffline}
                              </p>
                     </div>
                  </div>
                  <div className="device-badges">
                    {device.preferred && <span className="device-badge preferred">{t.connection.preferred}</span>}
                    {device.current && <span className="device-badge current">{t.connection.current}</span>}
                    {!device.enabled && <span className="device-badge paused">{t.ui.autoConnectOff}</span>}
                  </div>
                </div>
                <div className="device-foot">
                  {device.last_connected_at !== null && <p className="device-last-seen">{t.connection.lastConnected} · {new Date(device.last_connected_at).toLocaleString()}</p>}
                  <div className="device-actions">
                  {!device.preferred && (
                    <button className="btn" onClick={() => runDeviceAction(() => setPreferredDevice(device.id))}>
                      {t.connection.makePreferred}
                    </button>
                  )}
                  <button
                    className="btn"
                    onClick={() => runDeviceAction(() => setDeviceEnabled(device.id, !device.enabled))}
                  >
                    {device.enabled ? t.connection.pauseDevice : t.connection.enableDevice}
                  </button>
                  <button
                    className="btn danger"
                    disabled={device.current}
                    onClick={() => {
                      if (confirm(t.connection.forgetConfirm)) {
                        runDeviceAction(() => forgetDevice(device.id));
                      }
                    }}
                  >
                    {t.connection.forgetDevice}
                  </button>
                  </div>
                </div>
              </article>
            ))}
          </div>
        )}
      </Section>

      {showPairingGuide && (
        <div className="status-card" style={{ borderLeft: "3px solid #34c759" }}>
          <div>
            <strong>{t.connection.pairingGuideTitle}</strong>
            <p className="status-detail">{t.connection.pairingGuideStep1}</p>
            <p className="status-detail">{t.connection.pairingGuideStep2}</p>
          </div>
          <button className="btn" onClick={() => setShowGuide(true)}>{t.connection.diagnostics}</button>
        </div>
      )}
      {failed && <button className="btn" onClick={() => setShowGuide(true)}>{t.connection.diagnostics}</button>}
      {displayLink?.detail && <details className="diagnostic-details"><summary>{t.connection.details}</summary><p className="status-detail">{displayLink.detail}</p></details>}
      {showGuide && <ConnectionGuide onClose={() => setShowGuide(false)} />}
    </>
  );
}

/** 蓝牙电量图标：按电量比例填充、变色（≤20% 橙色警示），电量变化带过渡动画。 */
function Battery({ level }: { level: number }) {
  const clamped = Math.max(0, Math.min(100, level));
  const fillWidth = Math.max(1.2, (14.5 * clamped) / 100);
  const color = clamped <= 20 ? "#f0a13a" : "#34c759";
  return (
    <svg className="device-battery" width="21" height="11" viewBox="0 0 21 11" role="img" aria-label={`电量 ${clamped}%`}>
      <rect x="0.5" y="0.5" width="17" height="10" rx="2.5" fill="none" stroke="currentColor" strokeWidth="1" opacity="0.5" />
      <rect className="battery-fill" x="2" y="2" width={fillWidth} height="7" rx="1.2" fill={color} style={{ transition: "width 0.4s ease, fill 0.4s ease" }} />
      <path d="M18.8 3.4v4.2c1-.3 1.6-1 1.6-2.1s-.6-1.8-1.6-2.1Z" fill="currentColor" opacity="0.5" />
    </svg>
  );
}
