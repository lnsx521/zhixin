import { useEffect } from "react";
import { getConfig, onThemeChanged, type ThemeSetting } from "./api";

export function applyTheme(theme: ThemeSetting = "auto") {
  // auto 必须解析成 light/dark 写到属性上——组件里所有显式深色覆盖都挂在
  // :root[data-theme="dark"] 下，属性留 "auto" 会导致半边深色半边浅色。
  const resolved =
    theme === "auto"
      ? window.matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light"
      : theme;
  document.documentElement.dataset.theme = resolved;
}

/** 跟随系统时监听 OS 深浅切换，实时重解析。 */
export function watchSystemTheme() {
  const media = window.matchMedia("(prefers-color-scheme: dark)");
  const handler = () => {
    const current = document.documentElement.dataset.theme;
    void getConfig().then((config) => {
      if (config.general.theme === "auto") applyTheme("auto");
      else if (current !== config.general.theme) applyTheme(config.general.theme as ThemeSetting);
    }).catch(() => {});
  };
  media.addEventListener("change", handler);
  return () => media.removeEventListener("change", handler);
}

/** Popup windows load the saved preference and stay in sync while open. */
export function useSavedTheme() {
  useEffect(() => {
    let disposed = false;
    let receivedEvent = false;
    const unlisten = onThemeChanged((theme) => {
      receivedEvent = true;
      if (!disposed) applyTheme(theme);
    });
    void unlisten.then(async () => {
      const config = await getConfig();
      if (!disposed && !receivedEvent) applyTheme(config.general.theme);
    }).catch(() => {});
    return () => {
      disposed = true;
      void unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);
}
