import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUpdateLink, sendTestNotification, type Config } from "../../shared/api";
import { useT } from "../../shared/i18n";
import { Disclosure, NumberField, Row, TextList, Toggle } from "../components";
import { AppRules } from "./AppRules";
import { Otp } from "./Otp";

interface Props {
  config: Config;
  update: (mutate: (draft: Config) => void) => void;
}

/** 「通知」页：弹窗开关、外观、过滤。 */
export function Notifications({ config, update }: Props) {
  const t = useT();
  const notifications = config.notifications;
  const [testKind, setTestKind] = useState("sms");
  const [sending, setSending] = useState(false);
  const [sent, setSent] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const filterCount = Object.values(notifications.filter).reduce((total, entries) => total + entries.length, 0);

  // 常用 App 胶囊：点一下复制 bundle id，由用户决定粘到「排除」还是「只接受」。
  const quickApps = [
    { id: "com.apple.mobilephone", name: t.rules.phone },
    { id: "com.apple.mobilesms", name: t.rules.sms },
    { id: "com.tencent.xin", name: t.rules.wechat },
    { id: "com.tencent.mqq", name: t.rules.qq },
    { id: "com.tencent.wework", name: t.rules.workWechat },
    { id: "com.laiwang.DingTalk", name: t.rules.dingtalk },
    { id: "com.ss.iphone.lark", name: t.rules.feishu },
  ];
  const bundleIdLookupUrl = "https://commandnotfound.cn/tools/ios-app-bundle-id";
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const copyChip = async (id: string) => {
    try {
      await navigator.clipboard.writeText(id);
      setCopiedId(id);
      setTimeout(() => setCopiedId(null), 1200);
    } catch { /* 复制失败不打断 */ }
  };
  const openLookup = async () => {
    try { await navigator.clipboard.writeText(bundleIdLookupUrl); } catch { /* 复制失败不影响打开 */ }
    setCopiedId(bundleIdLookupUrl);
    setTimeout(() => setCopiedId(null), 1200);
    try { await openUpdateLink(bundleIdLookupUrl); } catch { /* 打开失败不打断 */ }
  };
  const quickChips = (
    <div className="known-apps">
      {quickApps.map((app) => <button key={app.id} className={`known-chip ${copiedId === app.id ? "active" : ""}`}
        title={`点击复制：${app.id}`} onClick={() => void copyChip(app.id)}>
        {copiedId === app.id ? "已复制" : app.name}
      </button>)}
      <button className={`known-chip ${copiedId === bundleIdLookupUrl ? "active" : ""}`}
        title={`打开浏览器：${bundleIdLookupUrl}`} onClick={() => void openLookup()}>
        {t.notifications.queryMore}
      </button>
    </div>
  );

  return (
    <>
      <Disclosure title={t.ui.popups} action={
          <Toggle
            checked={notifications.enabled}
            onChange={(value) => update((d) => void (d.notifications.enabled = value))}
          />
        }>
        <p className="connection-help">{t.ui.popupHint}</p>
        <Row label={t.notifications.duration} hint={t.notifications.durationHint}>
          <NumberField
            value={notifications.popup.duration_seconds}
            min={2}
            max={60}
            suffix={t.common.seconds}
            onChange={(value) =>
              update((d) => void (d.notifications.popup.duration_seconds = value))
            }
          />
        </Row>
        <Row label={t.notifications.width} hint={t.notifications.widthHint}>
          <NumberField
            value={notifications.popup.width}
            min={240}
            max={480}
            onChange={(value) =>
              update((d) => void (d.notifications.popup.width = value))
            }
          />
        </Row>
        <Row label={t.notifications.sound} hint={t.notifications.soundHint}>
          <Toggle
            checked={notifications.popup.sound}
            onChange={(value) =>
              update((d) => void (d.notifications.popup.sound = value))
            }
          />
        </Row>
        <Row label={t.notifications.soundFile} hint={t.notifications.soundFileHint}>
          <span className="sound-file-path">{notifications.popup.sound_file || t.notifications.soundFileDefault}</span>
          <button
            className="btn"
            onClick={() => {
              invoke<string | null>("pick_sound_file")
                .then((path) => {
                  if (path) update((d) => void (d.notifications.popup.sound_file = path));
                })
                .catch(() => {});
            }}
          >
            {t.notifications.pickSound}
          </button>
          {notifications.popup.sound_file && (
            <button
              className="btn"
              onClick={() => update((d) => void (d.notifications.popup.sound_file = null))}
            >
              {t.notifications.clearSound}
            </button>
          )}
        </Row>
        <Row label={t.notifications.maxVisible} hint={t.notifications.maxVisibleHint}>
          <NumberField
            value={notifications.popup.max_visible}
            min={1}
            max={10}
            onChange={(value) =>
              update((d) => void (d.notifications.popup.max_visible = value))
            }
          />
        </Row>
      <div className="local-test">
        <div><strong>{t.connection.testTitle}</strong><p>{t.connection.testDesc}</p>{error && <p className="bad-text" role="alert">{error}</p>}</div>
        <select className="select" value={testKind} onChange={(e) => setTestKind(e.target.value)} style={{ maxWidth: 180 }}>
          <option value="sms">短信 · 验证码</option>
          <option value="long_sms">长短信 · 全文阅读</option>
          <option value="call">来电</option>
          <option value="wechat">微信</option>
          <option value="mail">邮件</option>
          <option value="generic">通用</option>
        </select>
        <button className="btn" disabled={sending} onClick={async () => { setError(null); setSending(true); setSent(false); try { await sendTestNotification(testKind); setSending(false); setSent(true); setTimeout(() => setSent(false), 1500); } catch (err) { setError(String(err)); setSending(false); } }}>{sending ? t.connection.sending : sent ? t.connection.sent : t.connection.sendTest}</button>
      </div>
      </Disclosure>

      <Disclosure title={t.notifications.dndTitle}>
        <Row label={t.notifications.dndAsk} hint={t.notifications.dndAskHint}>
          <Toggle
            checked={notifications.dnd_ask_enabled}
            onChange={(value) => update((d) => void (d.notifications.dnd_ask_enabled = value))}
          />
        </Row>
        <Row label={t.notifications.dndRecover} hint={t.notifications.dndRecoverHint}>
          <NumberField
            value={notifications.dnd_recover_secs}
            min={5}
            max={300}
            suffix={t.common.seconds}
            onChange={(value) =>
              update((d) => void (d.notifications.dnd_recover_secs = Math.round(value)))
            }
          />
        </Row>
      </Disclosure>

      <AppRules config={config} update={update} />
      <Disclosure title={t.notifications.filterTitle} hint={filterCount ? t.ui.configured(filterCount) : undefined}>
        <p className="connection-help">{t.ui.filterNote}</p>

        <h3 className="filter-group-title">{t.notifications.quickApps}</h3>
        {quickChips}
        <h3 className="filter-group-title">{t.notifications.popupGroup}</h3>
        <p className="connection-help">{t.notifications.popupGroupDesc}</p>
        <Row label={t.notifications.excludeApps} hint={t.notifications.excludeAppsHint}>
          <TextList
            value={notifications.filter.exclude_apps}
            placeholder={"com.tencent.xin"}
            onChange={(value) => update((d) => void (d.notifications.filter.exclude_apps = value))}
          />
        </Row>
        <Row label={t.notifications.includeApps} hint={t.notifications.includeAppsHint}>
          <TextList
            value={notifications.filter.include_apps}
            placeholder={"com.apple.MobileSMS"}
            onChange={(value) => update((d) => void (d.notifications.filter.include_apps = value))}
          />
        </Row>
        <Row label={t.notifications.excludeKeywords} hint={t.notifications.excludeKeywordsHint}>
          <TextList
            value={notifications.filter.exclude_keywords}
            placeholder={t.notifications.excludeKeywords}
            onChange={(value) =>
              update((d) => void (d.notifications.filter.exclude_keywords = value))
            }
          />
        </Row>
        <Row label={t.notifications.includeKeywords} hint={t.notifications.includeKeywordsHint}>
          <TextList
            value={notifications.filter.include_keywords}
            onChange={(value) =>
              update((d) => void (d.notifications.filter.include_keywords = value))
            }
          />
        </Row>

        <h3 className="filter-group-title">{t.notifications.historyGroup}</h3>
        <p className="connection-help">{t.notifications.historyGroupDesc}</p>
        <Row label={t.notifications.keepHistoryApps} hint={t.notifications.keepHistoryHint}>
          <TextList
            value={notifications.history_keep_apps}
            placeholder={"com.tencent.xin"}
            onChange={(value) => update((d) => void (d.notifications.history_keep_apps = value))}
          />
        </Row>
        <Row label={t.notifications.excludeHistoryApps} hint={t.notifications.excludeHistoryHint}>
          <TextList
            value={notifications.history_exclude_apps}
            placeholder={"com.tencent.xin"}
            onChange={(value) => update((d) => void (d.notifications.history_exclude_apps = value))}
          />
        </Row>
        <Row label={t.notifications.historyTitle} hint={t.notifications.historyDesc}>
          <NumberField
            value={notifications.history_limit}
            min={0}
            max={3650}
            suffix="天"
            onChange={(value) =>
              update((d) => void (d.notifications.history_limit = Math.max(0, Math.min(3650, value))))
            }
          />
        </Row>
      </Disclosure>

      {/* 验证码是消息流程的下半段：消息进来 → 识别验证码 → 复制/填入。
          同一页承接，不再单独占一个导航项。 */}
      <Otp config={config} update={update} />

    </>
  );
}
