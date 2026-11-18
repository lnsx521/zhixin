import { useEffect, useRef, useState } from "react";
import {
  getSettingsMeta,
  openConfigDir,
  openLogsDir,
  saveBackgroundImage,
  setAutostart,
  type Config,
  type SettingsMeta,
  type ThemeSetting,
} from "../../shared/api";
import { useT } from "../../shared/i18n";
import { clearIconCache, listIconCache, openIconsDir, resetAppIcon, setAppIcon, type IconCacheEntry } from "../../shared/api";
import { NumberField, Row, Section, Toggle } from "../components";

interface Props {
  config: Config;
  update: (mutate: (draft: Config) => void) => void;
}

/** 「通用」页：自启、语言、文件位置、关于。 */
export function General({ config, update }: Props) {
  const t = useT();
  const [meta, setMeta] = useState<SettingsMeta | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [iconCache, setIconCache] = useState<IconCacheEntry[]>([]);
  const bgFileRef = useRef<HTMLInputElement>(null);
  const replaceTarget = useRef<IconCacheEntry | null>(null);
  const replaceInput = useRef<HTMLInputElement>(null);

  const onReplaceFile = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    const target = replaceTarget.current;
    if (!file || !target) return;
    try {
      const dataUrl = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(reader.result as string);
        reader.onerror = reject;
        reader.readAsDataURL(file);
      });
      await setAppIcon(target.app_id, target.name, dataUrl);
      await refreshIcons();
    } catch (err) { setError(String(err)); }
    event.target.value = "";
  };

  const refreshIcons = async () => {
    try { setIconCache(await listIconCache()); } catch { /* 忽略 */ }
  };

  const onPickBackground = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;
    try {
      await saveBackgroundImage(new Uint8Array(await file.arrayBuffer()));
      update((d) => void (d.general.background = "custom"));
    } catch (err) { setError(String(err)); }
    event.target.value = "";
  };

  useEffect(() => {
    getSettingsMeta().then(setMeta).catch(() => {});
    void refreshIcons();
  }, []);

  return (
    <>
      <input ref={replaceInput} type="file" accept="image/png" style={{ display: "none" }} onChange={(event) => { void onReplaceFile(event); }} />
      <Section narrow title={t.general.appearanceTitle}>
        <Row label={t.general.theme} hint={t.general.themeHint}>
          <select className="select" aria-label={t.general.theme}
            value={config.general.theme}
            onChange={(event) => update((draft) => {
              draft.general.theme = event.target.value as ThemeSetting;
            })}>
            <option value="auto">{t.general.langAuto}</option>
            <option value="light">{t.general.themeLight}</option>
            <option value="dark">{t.general.themeDark}</option>
          </select>
        </Row>
        <Row label={t.general.language} hint={t.general.languageHint}>
          <select
            className="select"
            value={config.general.language}
            onChange={(event) =>
              update((d) => void (d.general.language = event.target.value))
            }
          >
            <option value="auto">{t.general.langAuto}</option>
            <option value="zh">中文</option>
            <option value="en">English</option>
          </select>
        </Row>
        <Row label={t.general.background} hint={t.general.backgroundHint}>
          <div className="bg-picker" role="radiogroup" aria-label={t.general.background}>
            {([
              ["classic", t.general.bgClassic],
              ["blue", t.general.bgBlue],
              ["warm", t.general.bgWarm],
              ["mint", t.general.bgMint],
              ["custom", t.general.bgCustom],
            ] as const).map(([key, label]) => (
              <button
                key={key}
                role="radio"
                aria-checked={config.general.background === key}
                title={label}
                className={`bg-swatch ${config.general.background === key ? "selected" : ""}`}
                onClick={() => { if (key === "custom") bgFileRef.current?.click(); else update((d) => void (d.general.background = key)); }}
              >
                <span className={`bg-swatch-fill bg-${key}`}>{key === "custom" && <span className="bg-custom-plus">+</span>}</span>
              </button>
            ))}
            <input ref={bgFileRef} type="file" accept="image/*" style={{ display: "none" }} onChange={(e) => void onPickBackground(e)} />
          </div>
        </Row>
      </Section>
      <details className="icons-fold" style={{ borderTop: "0.5px solid var(--border)", marginTop: 12, paddingTop: 10 }}>
        <summary style={{ cursor: "pointer", display: "flex", alignItems: "center", justifyContent: "space-between" }}>
          <span style={{ fontSize: 13, fontWeight: 500, color: "var(--text)" }}>
            {t.general.iconsTitle}
            <span style={{ fontSize: 12, color: "var(--text-faint)", marginLeft: 8, fontWeight: 400 }}>{t.general.iconsDesc}</span>
          </span>
          <span onClick={(event) => event.stopPropagation()}>
            <Toggle
              checked={config.general.icon_fetch}
              onChange={(value) => update((d) => void (d.general.icon_fetch = value))}
            />
          </span>
        </summary>
        <div style={{ marginTop: 10 }}>
        <Row label={t.general.iconsOpenDir}>
          <button className="btn" onClick={() => { void openIconsDir().catch((err) => setError(String(err))); }}>{t.general.iconsOpenDir}</button>
        </Row>
        <Row label={t.general.iconsClear}>
          <button className="btn" onClick={async () => {
            try { await clearIconCache(); await refreshIcons(); } catch (err) { setError(String(err)); }
          }}>{t.general.iconsClear}</button>
        </Row>
        {iconCache.length === 0 && <Row label={t.general.iconsEmpty}><span /></Row>}
        {iconCache.map((entry) => (
          <Row key={entry.app_id} label={`${entry.name || entry.app_id}${entry.manual ? `（${t.general.iconsManual}）` : ""}`} hint={entry.app_id}>
            <button className="btn" onClick={() => { replaceTarget.current = entry; replaceInput.current?.click(); }}>{t.general.iconsReplace}</button>
            <button className="btn" onClick={async () => {
              try { await resetAppIcon(entry.app_id); await refreshIcons(); } catch (err) { setError(String(err)); }
            }}>{t.general.iconsReset}</button>
          </Row>
        ))}
        </div>
      </details>
      <Section narrow title={t.general.startupTitle}>
        <Row label={t.general.autostart} hint={t.general.autostartHint}>
          <Toggle checked={meta?.autostart_enabled ?? false} disabled={!meta || busy} onChange={async (value) => {
            setBusy(true); setError(null);
            try { const enabled = await setAutostart(value); setMeta((prev) => prev ? { ...prev, autostart_enabled: enabled } : prev); }
            catch (err) { setError(String(err)); }
            finally { setBusy(false); }
          }} />
        </Row>
        {error && <p className="bad-text" role="alert">{error}</p>}
      </Section>

      <Section title={t.general.lowBatteryTitle} description={t.general.lowBatteryDesc}>
        <Row label={t.general.lowBatteryNotify} hint={t.general.lowBatteryNotifyHint}>
          <Toggle
            checked={config.general.low_battery_notify}
            onChange={(value) => update((d) => void (d.general.low_battery_notify = value))}
          />
        </Row>
        <Row label={t.general.lowBatteryThreshold} hint={t.general.lowBatteryThresholdHint}>
          <NumberField
            value={config.general.low_battery_threshold}
            min={5}
            max={50}
            suffix="%"
            onChange={(value) =>
              update((d) => void (d.general.low_battery_threshold = Math.round(value)))
            }
          />
        </Row>
      </Section>
      <Section title={t.general.filesTitle}>
        <Row label={t.general.configFile} hint={meta?.config_path}>
          <button className="btn" onClick={() => openConfigDir()}>
            {t.general.openDir}
          </button>
        </Row>
        <Row label={t.general.logFile} hint={meta?.log_dir}>
          <button className="btn" onClick={() => openLogsDir()}>
            {t.general.openDir}
          </button>
        </Row>
      </Section>

    </>
  );
}
