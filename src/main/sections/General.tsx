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
import { Disclosure, NumberField, Row, Toggle } from "../components";

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
      await setAppIcon(target.app_id, dataUrl);
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
    // 后端在后台补齐旧缓存的官方 App 名，稍后回刷一次显示出来
    const timer = window.setTimeout(() => { void refreshIcons(); }, 3000);
    return () => window.clearTimeout(timer);
  }, []);

  return (
    <>
      <input ref={replaceInput} type="file" accept="image/png" style={{ display: "none" }} onChange={(event) => { void onReplaceFile(event); }} />
      <Disclosure title={t.general.appearanceTitle}>
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
      </Disclosure>
      <Disclosure
        title={t.general.iconsTitle}
        action={
          <Toggle
            checked={config.general.icon_fetch}
            onChange={(value) => update((d) => void (d.general.icon_fetch = value))}
          />
        }
      >
        <p className="connection-help">{t.general.iconsDesc}</p>
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
              <Row key={entry.app_id} label={`${entry.app_name || entry.app_id}${entry.manual ? `（${t.general.iconsManual}）` : ""}`} hint={entry.app_name ? entry.app_id : undefined}>
                <button className="btn" onClick={() => { replaceTarget.current = entry; replaceInput.current?.click(); }}>{t.general.iconsReplace}</button>
                <button className="btn" onClick={async () => {
                  try { await resetAppIcon(entry.app_id); await refreshIcons(); } catch (err) { setError(String(err)); }
                }}>{t.general.iconsReset}</button>
              </Row>
            ))}
      </Disclosure>
      <Disclosure
        title={t.general.startupTitle}
        action={
          <Toggle
            checked={meta?.autostart_enabled ?? false}
            disabled={!meta || busy}
            onChange={async (value) => {
              setBusy(true); setError(null);
              try { const enabled = await setAutostart(value); setMeta((prev) => prev ? { ...prev, autostart_enabled: enabled } : prev); }
              catch (err) { setError(String(err)); }
              finally { setBusy(false); }
            }}
          />
        }
      >
        <p className="connection-help">{t.general.autostartHint}</p>
        {error && <p className="bad-text" role="alert">{error}</p>}
        <Row label={t.general.autoCheckUpdateTitle}>
          <Toggle
            checked={config.general.auto_check_update}
            onChange={(value) => update((d) => void (d.general.auto_check_update = value))}
          />
        </Row>
      </Disclosure>

      <Disclosure
        title={t.general.lowBatteryTitle}
        action={
          <Toggle
            checked={config.general.low_battery_notify}
            onChange={(value) => update((d) => void (d.general.low_battery_notify = value))}
          />
        }
      >
        <p className="connection-help">{t.general.lowBatteryDesc}</p>
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
      </Disclosure>
      <Disclosure title={t.general.filesTitle}>
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
      </Disclosure>

    </>
  );
}
