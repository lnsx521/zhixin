import { useEffect, useState } from "react";
import { clearNotificationHistory, listNotificationHistory, type HistoryEntry } from "../../shared/api";
import { useT } from "../../shared/i18n";
import { Section, Row } from "../components";

/** 「历史消息」页：弹过的通知留档，可搜索/复制/清空。搜索框位于页面头部（App 注入）。 */
export function History({ query }: { query: string }) {
  const t = useT();
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [error, setError] = useState<string | null>(null);

  const refresh = async (needle: string) => {
    try { setEntries(await listNotificationHistory(needle, 100)); }
    catch (err) { setError(String(err)); }
  };

  useEffect(() => { void refresh(query); }, [query]);

  const time = (ms: number) => new Date(ms).toLocaleString();

  return (
    <>
      <Section narrow title={t.history.title}>
        {error && <p className="bad-text">{error}</p>}
        {entries.length === 0 && <p className="hint-text">{query ? t.history.noResults : t.history.empty}</p>}
        <div className="history-list">
          {entries.map((entry) => (
            <article className="history-card" key={`${entry.received_at}-${entry.origin}`}>
              <div className="history-head">
                <strong>{entry.origin}</strong>
                <span className="history-time">{time(entry.received_at)}</span>
              </div>
              <p className="history-body">{entry.body}</p>
              {entry.code && <button className="btn" onClick={() => { void navigator.clipboard.writeText(entry.code!); }}>{t.history.copy(entry.code)}</button>}
            </article>
          ))}
        </div>
        <Row label={t.history.manage}>
          <button className="btn danger" onClick={async () => {
            if (!confirm(t.history.clearConfirm)) return;
            try { await clearNotificationHistory(); await refresh(query); } catch (err) { setError(String(err)); }
          }}>{t.history.clear}</button>
        </Row>
      </Section>
    </>
  );
}
