import { useEffect, useMemo, useState } from "react";
import { getAppIcon, getLinkState, listNotificationHistory, openAppById, type HistoryEntry } from "../../shared/api";
import { brandAsset } from "../../shared/brandIcons";
import { Icon } from "../../shared/Icon";

type Item = HistoryEntry & { day: string; timeText: string };

function dayGroup(receivedAt: number): string {
  const now = new Date();
  const d = new Date(receivedAt);
  const sameDay = now.getFullYear() === d.getFullYear() && now.getMonth() === d.getMonth() && now.getDate() === d.getDate();
  const yesterday = new Date(now.getTime() - 86400000);
  const isYesterday = yesterday.getMonth() === d.getMonth() && yesterday.getDate() === d.getDate() && yesterday.getFullYear() === d.getFullYear();
  if (sameDay) return "今天";
  if (isYesterday) return "昨天";
  return `${d.getMonth() + 1}月${d.getDate()}日`;
}

export function HomeView({ onOpenSettings }: { onOpenSettings: () => void }) {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [link, setLink] = useState<{ device_name: string | null; battery: number | null; connected: boolean } | null>(null);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Item | null>(null);
  const [icons, setIcons] = useState<Record<string, string>>({});

  useEffect(() => {
    void listNotificationHistory("", 50)
      .then((rows) => {
        setEntries(rows);
        const first = rows[0];
        if (first) setSelected({ ...first, day: dayGroup(first.received_at), timeText: fmtTime(first.received_at) });
      })
      .catch(() => {});
    const tick = () => void getLinkState().then((s) => setLink({ device_name: s.device_name, battery: s.battery, connected: s.ready || s.state === "subscribed" })).catch(() => {});
    tick();
    const timer = window.setInterval(tick, 5000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    (Object.keys(brandAssets(entries)) as string[]).forEach((id) => {
      if (icons[id]) return;
      getAppIcon(id).then((url) => { if (url) setIcons((prev) => ({ ...prev, [id]: url })); }).catch(() => {});
    });
  }, [entries]);

  const items: Item[] = useMemo(
    () =>
      entries.map((entry) => ({
        ...entry,
        day: dayGroup(entry.received_at),
        timeText: fmtTime(entry.received_at),
      })),
    [entries],
  );
  const filtered = query
    ? items.filter((it) => it.origin.includes(query) || it.body.includes(query))
    : items;
  const groups: { day: string; rows: Item[] }[] = [];
  for (const it of filtered) {
    const last = groups[groups.length - 1];
    if (last && last.day === it.day) last.rows.push(it);
    else groups.push({ day: it.day, rows: [it] });
  }

  return (
    <div className="home">
      <div className="home-topbar">
        <span className="home-topbar-app">知信</span>
        <span className="home-topbar-ver">v1.1.1</span>
      </div>
      <div className="home-main">
      <aside className="home-side">
        <div className="home-top">
          <div className="home-phone-row">
            <span className="home-phone-icon"><Icon name="phone" /></span>
            <div className="home-phone-col">
              <div className="home-phone-line">
                <span className="home-phone">{link?.device_name ?? "未连接手机"}</span>
                <span className="home-battery">
                  <svg viewBox="0 0 28 14" style={{ width: 24, height: 12 }}>
                    <rect x="0.5" y="0.5" width="23" height="13" rx="3.5" fill="none" stroke="var(--text-faint)" />
                    <rect x="2" y="2" width={Math.max(0, ((link?.battery ?? 0) / 100) * 20)} height="11" rx="2.5" fill={(link?.battery ?? 0) > 20 ? "#34C759" : "#E24B4A"} />
                    <rect x="24.5" y="4.5" width="2.5" height="5" rx="1" fill="var(--text-faint)" />
                  </svg>
                  <span style={{ marginLeft: 4 }}>{link?.battery ?? "–"}%</span>
                </span>
              </div>
              <span className="home-bt" style={{ color: link?.connected ? "#1a7f37" : "var(--text-secondary)" }}>
                {link?.connected ? "● 已连接" : "○ 未连接"}
              </span>
            </div>
          </div>
        </div>
        <div className="home-search">
          <input className="home-search-input" placeholder="搜索消息" value={query} onChange={(e) => setQuery(e.target.value)} />
        </div>
        <div className="home-list">
          {groups.map((g) => (
            <div key={g.day}>
              <div className="home-day">{g.day}</div>
              {g.rows.map((it) => {
                const key = `${it.received_at}-${it.origin}`;
                const asset = brandAsset(it.app_id);
                return (
                  <div
                    key={key}
                    className={`home-msg ${selected?.received_at === it.received_at && selected.origin === it.origin ? "selected" : ""}`}
                    onClick={() => setSelected(it)}
                  >
                    <div className="home-msg-icon">
                      {asset ? <img src={asset} alt="" /> : icons[it.app_id ?? ""] ? <img src={icons[it.app_id ?? ""]} alt="" /> : <img src="/brands/generic.png" alt="" />}
                    </div>
                    <div className="home-msg-texts">
                      <div className="home-msg-line1">
                        <span className="home-msg-origin">{it.origin}</span>
                        <span className="home-msg-time">{it.timeText}</span>
                      </div>
                      <div className="home-msg-snippet">{it.body}</div>
                    </div>
                  </div>
                );
              })}
            </div>
          ))}
          {filtered.length === 0 && <p className="home-empty">暂无消息</p>}
        </div>
        <div className="home-foot">
          <div className="home-setbtn" onClick={onOpenSettings}>
            <span className="home-gear">⚙</span>设置
          </div>
        </div>
      </aside>
      <section className="home-read">
        {selected ? (
          <>
            <div className="home-read-head">
              <span className="home-read-title">{selected.origin}</span>
              <span className="home-read-meta">{selected.timeText} · {selected.app_id === "com.apple.mobilephone" ? "来电" : "消息"}</span>
            </div>
            <div className="home-read-body">
              <div className="imsg-bubble">
                <p className="imsg-text">{selected.body}</p>
                <small className="imsg-time">{selected.timeText}</small>
              </div>
              <div className="home-read-actions">
                {selected.code && (
                  <button
                    className="toast-code"
                    onClick={async (e) => { e.stopPropagation(); await navigator.clipboard.writeText(selected.code!); }}
                  >
                    点击复制 {selected.code}
                  </button>
                )}
                <button className="imsg-copyall" onClick={async (e) => { e.stopPropagation(); await navigator.clipboard.writeText(selected.body); }}>
                  复制全文
                </button>
                {selected.app_id && (
                  <button className="imsg-copyall" onClick={async (e) => { e.stopPropagation(); await openAppById(selected.app_id!).catch(() => {}); }}>
                    打开应用
                  </button>
                )}
              </div>
            </div>
          </>
        ) : (
          <div className="home-read-empty">选择左侧消息查看全文</div>
        )}
      </section>
      </div>
    </div>
  );
}

function fmtTime(receivedAt: number): string {
  const d = new Date(receivedAt);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

function brandAssets(entries: HistoryEntry[]): Record<string, string | null> {
  const map: Record<string, string | null> = {};
  entries.forEach((entry) => { if (entry.app_id) map[entry.app_id] = brandAsset(entry.app_id); });
  return map;
}
