import { useEffect, useState } from "react";
import { clearNotificationHistory, getAppIcon, historyExpand, listNotificationHistory, type HistoryEntry } from "../../shared/api";
import { brandAsset } from "../../shared/brandIcons";
import { listen } from "@tauri-apps/api/event";
import { useT } from "../../shared/i18n";
import { Section, Row } from "../components";

/** 「历史消息」页：弹过的通知留档，可搜索/复制/清空。搜索框位于页面头部（App 注入）。 */
export function History({ query }: { query: string }) {
  const t = useT();
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [confirmClear, setConfirmClear] = useState(false);

  const refresh = async (needle: string) => {
    try { setEntries(await listNotificationHistory(needle, 100)); }
    catch (err) { setError(String(err)); }
  };

  useEffect(() => { void refresh(query); }, [query]);

  // 新通知入库 / 清空历史时后端广播，实时刷新列表
  useEffect(() => {
    const unlisten = listen("history-updated", () => { void refresh(query); });
    return () => { unlisten.then((f) => f()); };
  }, [query]);

  // 每条通知前显示来源 App 图标：联网缓存/手动设置 > 内置品牌 > 纸飞机兜底
  const [cachedIcons, setCachedIcons] = useState<Record<string, string>>({});
  useEffect(() => {
    const ids = [...new Set(entries.map((e) => e.app_id).filter(Boolean))] as string[];
    ids.forEach((id) => {
      if (cachedIcons[id]) return;
      getAppIcon(id).then((url) => { if (url) setCachedIcons((prev) => ({ ...prev, [id]: url })); }).catch(() => {});
    });
  }, [entries]);

  const time = (ms: number) => new Date(ms).toLocaleString();

  return (
    <>
      <Section narrow title="">
        {error && <p className="bad-text">{error}</p>}
        {entries.length === 0 && <p className="hint-text">{query ? t.history.noResults : t.history.empty}</p>}
        <div className="history-list">
          {entries.map((entry) => (
            <article
              className="history-card"
              key={`${entry.received_at}-${entry.origin}`}
              onClick={() => { void historyExpand(entry).catch((err) => setError(String(err))); }}
            >
              <div className="history-icon">
                {brandAsset(entry.app_id)
                  ? <img src={brandAsset(entry.app_id)!} alt="" />
                  : cachedIcons[entry.app_id ?? ""]
                    ? <img src={cachedIcons[entry.app_id ?? ""]} alt="" />
                    : <img src="/brands/generic.png" alt="" style={{ opacity: 0.45 }} />}
              </div>
              <div className="history-main">
              <div className="history-head">
                <strong>{entry.origin}</strong>
                <span className="history-time">{time(entry.received_at)}</span>
              </div>
              <p className="history-body">{entry.body}<span className="history-more"> 查看全文</span></p>
              </div>
            </article>
          ))}
        </div>
        <Row label={t.history.manage}>
          <button className={`btn danger ${confirmClear ? "confirming" : ""}`} onClick={async () => {
            if (!confirmClear) { setConfirmClear(true); return; }
            setConfirmClear(false);
            try { await clearNotificationHistory(); await refresh(query); } catch (err) { setError(String(err)); }
          }}>{confirmClear ? "确认清空？" : t.history.clear}</button>
        </Row>
      </Section>
    </>
  );
}
