import { useEffect, useRef, useState } from "react";
import { getBackgroundImage, getConfig, openTransferFolder, saveConfig, type Config } from "../shared/api";
import { LangContext, resolveLang, useT } from "../shared/i18n";
import appIcon from "../shared/app-icon.svg";
import { Connection } from "./sections/Connection";
import { Notifications } from "./sections/Notifications";
import { General } from "./sections/General";
import { Proximity } from "./sections/Proximity";
import { About } from "./sections/About";
import { Network } from "./sections/Network";
import { Transfer } from "./sections/Transfer";
import { Icon, type IconName } from "../shared/Icon";
import { applyTheme } from "../shared/theme";
import { AutoSave, type SaveStatus } from "../shared/autosave";
import { Titlebar } from "./Titlebar";
import { useUpdater } from "./useUpdater";
import { UpdateNotice, UpdateDialog } from "./sections/SoftwareUpdate";

type Page = "connection" | "notifications" | "proximity" | "network" | "transfer" | "general" | "about";

const NAV_ICONS: Record<Page, IconName> = {
  connection: "phone",
  notifications: "bell",
  proximity: "lock",
  network: "external",
  transfer: "folder",
  general: "settings",
  about: "info",
};

export function App() {
  const [page, setPage] = useState<Page>("connection");
  const [config, setConfig] = useState<Config | null>(null);
  const [saveStatus, setSaveStatus] = useState<SaveStatus>("idle");
  const [saveError, setSaveError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const configRef = useRef<Config | null>(null);
  const mounted = useRef(false);
  const [autosave] = useState(() => new AutoSave<Config>(saveConfig, (status, message) => {
    if (!mounted.current) return;
    setSaveStatus(status);
    setSaveError(message ?? null);
  }));

  useEffect(() => {
    mounted.current = true;
    const flush = () => { void autosave.flush(); };
    const onVisibility = () => { if (document.hidden) flush(); };
    window.addEventListener("blur", flush);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      mounted.current = false;
      window.removeEventListener("blur", flush);
      document.removeEventListener("visibilitychange", onVisibility);
      flush();
    };
  }, [autosave]);

  useEffect(() => {
    applyTheme(config?.general.theme);
  }, [config?.general.theme]);

  useEffect(() => {
    const bg = config?.general.background;
    if (bg) document.documentElement.dataset.bg = bg;
  }, [config?.general.background]);

  // 自定义背景图：从后端读原文件 → object URL 铺满窗口；其他主题清掉。
  useEffect(() => {
    let url: string | null = null;
    if (config?.general.background === "custom") {
      getBackgroundImage()
        .then((buf) => {
          if (!buf || buf.byteLength === 0) return;
          url = URL.createObjectURL(new Blob([buf]));
          document.body.style.backgroundImage = `url(${url})`;
          document.body.style.backgroundSize = "cover";
          document.body.style.backgroundPosition = "center";
          document.body.style.backgroundAttachment = "fixed";
        })
        .catch(() => {});
    } else {
      document.body.style.backgroundImage = "";
    }
    return () => { if (url) URL.revokeObjectURL(url); };
  }, [config?.general.background]);

  useEffect(() => {
    let disposed = false;
    getConfig()
      .then((loaded) => {
        if (disposed) return;
        configRef.current = loaded;
        setConfig(loaded);
      })
      .catch((err) => { if (!disposed) setError(String(err)); });
    return () => { disposed = true; };
  }, []);

  /** 以「改草稿」的方式更新配置 */
  const update = (mutate: (draft: Config) => void) => {
    if (!configRef.current) return;
    const draft = structuredClone(configRef.current);
    mutate(draft);
    if (JSON.stringify(draft) === JSON.stringify(configRef.current)) return;
    configRef.current = draft;
    setConfig(draft);
    autosave.schedule(draft);
  };

  const lang = resolveLang(config?.general.language ?? "auto");

  return (
    <LangContext.Provider value={lang}>
      <Shell
        page={page}
        setPage={setPage}
        config={config}
        update={update}
        saveStatus={saveStatus}
        saveError={saveError}
        error={error}
        retry={() => { void autosave.flush(); }}
        beforeClose={() => autosave.flush()}
        beforeInstall={() => autosave.flushBeforeExit()}
      />
    </LangContext.Provider>
  );
}

function Shell(props: {
  page: Page;
  setPage: (page: Page) => void;
  config: Config | null;
  update: (mutate: (draft: Config) => void) => void;
  saveStatus: SaveStatus;
  saveError: string | null;
  error: string | null;
  retry: () => void;
  beforeClose: () => Promise<void>;
  beforeInstall: () => Promise<void>;
}) {
  const t = useT();
  const updater = useUpdater();
  const { page, setPage, config, update, saveStatus, saveError, error, retry, beforeClose } = props;

  const nav: { key: Page; label: string }[] = [
    { key: "connection", label: t.nav.connection },
    { key: "network", label: t.nav.network },
    { key: "transfer", label: t.nav.transfer },
    { key: "notifications", label: t.nav.notifications },
    { key: "proximity", label: t.nav.proximity },
    { key: "general", label: t.nav.general },
    { key: "about", label: t.nav.about },
  ];

  return (
    <div className="app-shell">
      <Titlebar beforeClose={beforeClose} />

      <div className="layout">
        <aside className="sidebar">
          <div className="brand">
            <img src={appIcon} width="38" height="38" alt="" />
            <div className="brand-text">
              <span className="brand-name">知信</span>
              <span className="brand-sub">{t.shell.tagline}</span>
            </div>
          </div>
          <nav aria-label={t.shell.navigation}>
            {nav.map((item) => (
              <button
                key={item.key}
                className={`nav-item ${page === item.key ? "active" : ""}`}
                onClick={() => setPage(item.key)}
                aria-current={page === item.key ? "page" : undefined}
              >
                <Icon name={NAV_ICONS[item.key]} />
                {item.label}
              </button>
            ))}
          </nav>
          <div className="sidebar-foot">
            <UpdateNotice updater={updater} />
            <div className={`autosave-status ${saveStatus === "error" ? "bad-text" : ""}`} role="status">
              {saveStatus === "idle" ? t.common.autoSave : saveStatus === "saved" ? t.common.saved : saveStatus === "error" ? t.common.saveFailed : t.common.saving}
            </div>
          </div>
        </aside>

        <main className="content">
          <header className="page-header">
            <div>
              <h1>{t.nav[page]}</h1>
              <p>{t.shell[page]}</p>
            </div>
            {page === "transfer" && (
              <div className="page-actions">
                <button className="btn" onClick={() => void openTransferFolder().catch(() => {})}>{t.general.openDir}</button>
              </div>
            )}
          </header>
          {error && <div className="error-banner">{error}</div>}
          {saveError && <div className="error-banner" role="alert">
            <span>{t.common.saveFailed}: {saveError}</span>
            <button className="btn" onClick={retry}>{t.common.retry}</button>
          </div>}

          {config ? (
            <>
              {page === "connection" && <Connection />}
              {page === "notifications" && <Notifications config={config} update={update} />}
              {page === "proximity" && <Proximity config={config} update={update} />}
              {page === "network" && <Network />}
              {page === "transfer" && <Transfer />}
              {page === "general" && <General config={config} update={update} />}
              {page === "about" && <About updater={updater} />}
            </>
          ) : (
            <p className="loading">{t.common.loading}</p>
          )}
        </main>
      </div>
      {updater.open && <UpdateDialog updater={updater} beforeInstall={props.beforeInstall} />}
    </div>
  );
}
