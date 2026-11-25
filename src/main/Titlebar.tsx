import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useT } from "../shared/i18n";
import { getSettingsMeta } from "../shared/api";

export function Titlebar({ beforeClose, closeOnly = false }: { beforeClose: () => Promise<void>; closeOnly?: boolean }) {
  const t = useT();
  const [maximized, setMaximized] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState("");

  useEffect(() => {
    let disposed = false;
    const appWindow = getCurrentWindow();
    const refresh = async () => {
      const value = await appWindow.isMaximized();
      if (!disposed) setMaximized(value);
    };
    const unlisten = appWindow.onResized(() => { void refresh().catch(() => {}); });
    void refresh().catch(() => {});
    return () => {
      disposed = true;
      void unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  // 版本号从后端动态读（跟关于页同源），不再硬编码——避免发版后标题栏与关于页对不上
  useEffect(() => {
    let disposed = false;
    void getSettingsMeta().then((meta) => { if (!disposed) setVersion(meta.version); }).catch(() => {});
    return () => { disposed = true; };
  }, []);

  const run = async (action: () => Promise<void>) => {
    setError(null);
    try {
      await action();
    } catch (reason) {
      setError(String(reason));
    }
  };

  return (
    <>
      <header className="titlebar">
        {/* Tauri handles native dragging and double-click maximize on this region.
            Keep nested decoration pointer-transparent so the entire region works. */}
        <span className="brand">
          <span className="brand-name">知信</span>
          {version && <span className="brand-ver">v{version}</span>}
        </span>
        <div className="titlebar-drag" data-tauri-drag-region />
        <div className="window-controls">
          {!closeOnly && (<>
          <button className="window-control" aria-label={t.window.minimize} title={t.window.minimize}
            onClick={() => { void run(() => getCurrentWindow().minimize()); }}>
            <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M1 6h10" /></svg>
          </button>
          <button className="window-control" aria-label={maximized ? t.window.restore : t.window.maximize}
            title={maximized ? t.window.restore : t.window.maximize}
            onClick={() => { void run(() => getCurrentWindow().toggleMaximize()); }}>
            <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
              {maximized ? <path d="M3.5 3.5v-2h7v7h-2M1.5 3.5h7v7h-7Z" /> : <rect x="1.5" y="1.5" width="9" height="9" />}
            </svg>
          </button>
          </>)}
          <button className="window-control close" aria-label={t.window.close} title={t.window.close}
            onClick={() => { void run(async () => { await beforeClose(); await getCurrentWindow().close(); }); }}>
            <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="m1.5 1.5 9 9m0-9-9 9" /></svg>
          </button>
        </div>
      </header>
      {error && <div className="window-error" role="alert">{t.window.error}: {error}</div>}
    </>
  );
}
