import { useEffect, useState } from "react";
import { getSettingsMeta, openProjectLink, type ProjectLink } from "../../shared/api";
import { Icon } from "../../shared/Icon";
import { useT } from "../../shared/i18n";
import appIcon from "../../shared/app-icon.svg";
import { SoftwareUpdate } from "./SoftwareUpdate";
import type { Updater } from "../useUpdater";

function Badge({ src, label }: { src: string; label: string }) {
  const [failed, setFailed] = useState(false);
  return failed ? <span className="about-badge-fallback">{label}</span>
    : <img className="about-badge" src={src} alt={label} referrerPolicy="no-referrer" onError={() => setFailed(true)} />;
}

export function About({ updater }: { updater: Updater }) {
  const t = useT();
  const [version, setVersion] = useState("…");
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let disposed = false;
    void getSettingsMeta().then((meta) => { if (!disposed) setVersion(meta.version); })
      .catch((err) => { if (!disposed) setError(String(err)); });
    return () => { disposed = true; };
  }, []);
  const open = async (target: ProjectLink) => {
    setError(null);
    try { await openProjectLink(target); } catch (err) { setError(String(err)); }
  };
  return <div className="about-page">
    <section className="about-hero" aria-label="知信">
      <div className="about-identity">
        <img className="about-app-icon" src={appIcon} alt="" />
        <div className="about-hero-content">
          <h2>知信 <span className="about-version">v{version}</span></h2>
          <p>{t.about.tagline}</p>
          <div className="about-actions">
            <button className="btn about-primary" onClick={() => void open("repository")}><Icon name="external" size={16} />{t.about.repository}</button>
            <button className="btn about-link" onClick={() => void open("issues")}>{t.about.feedback}<Icon name="external" size={14} /></button>
          </div>
        </div>
      </div>
      <div style={{ borderTop: "0.5px solid rgba(128,128,128,0.3)", margin: "12px 0 0", paddingTop: "12px" }}>
        <p className="about-maintainer-line" style={{ margin: 0, fontSize: 13, cursor: "pointer" }} onClick={() => void open("maintainer")} role="link" tabIndex={0}>
          {t.about.maintainer}：<b style={{ color: "#1D9E75" }}>lnsx521</b>
        </p>
      </div>
      <div className="about-support" style={{ borderTop: "none", marginTop: 8 }}><p className="about-community-note"><Icon name="star" size={14} />{t.about.starHint}</p>
        <button className="about-stars" aria-label={t.about.starAction} onClick={() => void open("repository")}>
          <Badge src="https://img.shields.io/github/stars/lnsx521/zhixin?style=flat-square&label=Stars&color=737373" label={t.about.starsBadge} />
        </button>
      </div>
    </section>
    <section className="about-update" aria-label={t.updater.title}>
      <SoftwareUpdate updater={updater} />
    </section>
    <footer className="about-footer"><p>{t.about.privacy}</p><span>© 知信 · 基于 SmsPop（fxaxg/sms-pop-rs）二次开发 · MIT License</span></footer>
    {error && <p className="bad-text" role="alert">{error}</p>}
  </div>;
}
