import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listKnownApps, sendTestNotification, type Config, type KnownApp } from "../../shared/api";
import { useT } from "../../shared/i18n";
import { Disclosure, NumberField, Row, Section, TextList, Toggle } from "../components";
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
  const [known, setKnown] = useState<KnownApp[]>([]);
  useEffect(() => {
    listKnownApps().then(setKnown).catch(() => {});
  }, []);
  const filterCount = Object.values(notifications.filter).reduce((total, entries) => total + entries.length, 0);

  return (
    <>
      <Section title={t.ui.popups} description={t.ui.popupHint} action={
          <Toggle
            checked={notifications.enabled}
            onChange={(value) => update((d) => void (d.notifications.enabled = value))}
          />
        }>
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
      </Section>

      <AppRules config={config} update={update} />
      <Disclosure title={t.notifications.filterTitle} hint={filterCount ? t.ui.configured(filterCount) : t.ui.unset}>
        <p className="connection-help">{t.ui.filterNote}</p>
        <p className="connection-help">{t.notifications.filterDesc}</p>
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
        {known.length > 0 && (
          <Row label={t.notifications.knownApps} hint={t.notifications.knownAppsHint}>
            <div className="known-apps">
              {known.map((app) => {
                const active = notifications.filter.include_apps.includes(app.id);
                return (
                  <button
                    key={app.id}
                    className={`known-chip ${active ? "active" : ""}`}
                    title={app.id}
                    onClick={() =>
                      update((d) => {
                        const list = d.notifications.filter.include_apps;
                        const index = list.indexOf(app.id);
                        if (index >= 0) list.splice(index, 1);
                        else list.push(app.id);
                      })
                    }
                  >
                    {app.name}
                  </button>
                );
              })}
            </div>
          </Row>
        )}
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
      </Disclosure>

      {/* 验证码是消息流程的下半段：消息进来 → 识别验证码 → 复制/填入。
          同一页承接，不再单独占一个导航项。 */}
      <Otp config={config} update={update} />

    </>
  );
}
