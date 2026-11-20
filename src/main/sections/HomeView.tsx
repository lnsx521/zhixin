import { useEffect, useMemo, useState } from "react";
import { getAppIcon, getLinkState, listNotificationHistory, openAppById, type HistoryEntry } from "../../shared/api";
import { brandAsset } from "../../shared/brandIcons";
import { Icon } from "../../shared/Icon";
import { listen } from "@tauri-apps/api/event";

type Item = HistoryEntry & { day: string; timeText: string };
type Conv = { kind: "conv"; key: string; origin: string; app_id: string | null; items: Item[] };
type Row = { kind: "single"; item: Item } | Conv;

interface Props {
  onOpenSettings: () => void;
}

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

/** 只有微信消息按会话合并（来电/验证码/其他应用都保持单条） */
function isChatEntry(e: HistoryEntry): boolean {
  return e.category !== 1 && !e.code && /tencent\.xin|wechat/i.test(e.app_id ?? "");
}

function convKeyOf(e: HistoryEntry): string {
  return `${e.app_id ?? ""}|${e.origin}`;
}

/** 列表行构建：聊天消息按会话合并（保留最新一条的位置），其余单条 */
function buildRows(rows: Item[]): Row[] {
  const map = new Map<string, Conv>();
  const out: Row[] = [];
  for (const it of rows) {
    if (!isChatEntry(it)) {
      out.push({ kind: "single", item: it });
      continue;
    }
    const key = convKeyOf(it);
    const conv = map.get(key);
    if (conv) {
      conv.items.push(it);
    } else {
      const created: Conv = { kind: "conv", key, origin: it.origin, app_id: it.app_id, items: [it] };
      map.set(key, created);
      out.push(created);
    }
  }
  return out;
}

/** 从消息体提取发件人（微信格式 "xiaoting : 在说了"），失败返回原文 */
function splitSender(body: string): { sender: string | null; text: string } {
  if (/^https?:/i.test(body)) return { sender: null, text: body };
  const m = body.match(/^([^:：\n]{1,16})\s*[:：]\s+/);
  if (m && /[^\d]/.test(m[1])) return { sender: m[1].trim(), text: body.slice(m[0].length) };
  return { sender: null, text: body };
}

export function HomeView({ onOpenSettings }: Props) {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [link, setLink] = useState<{ device_name: string | null; battery: number | null; connected: boolean } | null>(null);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Item | null>(null);
  const [chat, setChat] = useState<Conv | null>(null);
  const [chatMsgs, setChatMsgs] = useState<Item[]>([]);
  const [icons, setIcons] = useState<Record<string, string>>({});

  const openSingle = (it: Item) => {
    setChat(null);
    setSelected(it);
  };

  const openConv = (conv: Conv) => {
    setSelected(null);
    setChat(conv);
    setChatMsgs(conv.items);
    // 拉更长的历史补全聊天记录，按时间正序排列
    void listNotificationHistory(conv.origin, 200)
      .then((rows) => {
        const msgs = rows
          .filter((e) => isChatEntry(e) && e.origin === conv.origin && (e.app_id ?? "") === (conv.app_id ?? ""))
          .map((e) => ({ ...e, day: dayGroup(e.received_at), timeText: fmtTime(e.received_at) }))
          .sort((a, b) => a.received_at - b.received_at);
        if (msgs.length) setChatMsgs(msgs);
      })
      .catch(() => {});
  };

  // 点击小弹窗通知 → 主窗口自动定位：聊天消息打开所属会话，其余定位单条。
  useEffect(() => {
    void listen<HistoryEntry>("toast-clicked", (event) => {
      const p = event.payload;
      if (!p.received_at || !p.body) return;
      const match = entries.find(
        (e) => e.received_at === p.received_at && e.origin === p.origin && e.body === p.body,
      );
      if (!match) return;
      const item: Item = { ...match, day: dayGroup(match.received_at), timeText: fmtTime(match.received_at) };
      if (isChatEntry(match)) {
        const conv: Conv = { kind: "conv", key: convKeyOf(match), origin: match.origin, app_id: match.app_id, items: [item] };
        openConv(conv);
      } else {
        openSingle(item);
      }
      const el = document.querySelector(`[data-entry-key="${encodeKey(match.received_at, match.origin)}"]`);
      if (el) el.scrollIntoView({ behavior: "smooth", block: "center" });
    }).catch(() => {});
  }, [entries]);

  useEffect(() => {
    void listNotificationHistory("", 50)
      .then((rows) => {
        setEntries(rows);
        const first = rows[0];
        if (first) {
          const item: Item = { ...first, day: dayGroup(first.received_at), timeText: fmtTime(first.received_at) };
          if (isChatEntry(first)) openConv({ kind: "conv", key: convKeyOf(first), origin: first.origin, app_id: first.app_id, items: [item] });
          else openSingle(item);
        }
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
  // 先按天聚组，再对整组做会话合并（buildRows 的合并映射必须跨整组共享，
  // 逐条调用会各自新建映射导致合并失效）
  const dayGroups: { day: string; rows: Item[] }[] = [];
  for (const it of filtered) {
    const last = dayGroups[dayGroups.length - 1];
    if (last && last.day === it.day) last.rows.push(it);
    else dayGroups.push({ day: it.day, rows: [it] });
  }
  const groups = dayGroups.map((g) => ({ day: g.day, rows: buildRows(g.rows) }));

  return (
    <div className="home">
      <div className="home-main">
      <aside className="home-side">
        <div className="home-top">
          <div className="home-phone-row">
            <span className="home-phone-icon"><Icon name="phone" /></span>
            <div className="home-phone-col">
              <div className="home-phone-line">
                <span className="home-phone-wrap" data-tip={link?.device_name || undefined}>
                  <span className="home-phone">{link?.device_name ?? "未连接手机"}</span>
                </span>
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
              {g.rows.map((row) => {
                if (row.kind === "single") {
                  const it = row.item;
                  const key = `${it.received_at}-${it.origin}`;
                  const asset = brandAsset(it.app_id);
                  return (
                    <div
                      key={key}
                      data-entry-key={encodeKey(it.received_at, it.origin)}
                      className={`home-msg ${selected?.received_at === it.received_at && selected.origin === it.origin && !chat ? "selected" : ""}`}
                      onClick={() => openSingle(it)}
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
                }
                const conv = row;
                const latest = conv.items[0];
                const asset = brandAsset(conv.app_id);
                const isActive = chat?.key === conv.key;
                return (
                  <div
                    key={conv.key}
                    data-entry-key={encodeKey(latest.received_at, latest.origin)}
                    className={`home-msg ${isActive ? "selected" : ""}`}
                    onClick={() => openConv(conv)}
                  >
                    <div className="home-msg-icon">
                      {asset ? <img src={asset} alt="" /> : icons[conv.app_id ?? ""] ? <img src={icons[conv.app_id ?? ""]} alt="" /> : <img src="/brands/generic.png" alt="" />}
                    </div>
                    <div className="home-msg-texts">
                      <div className="home-msg-line1">
                        <span className="home-msg-origin">{conv.origin}</span>
                        {conv.items.length > 1 && <span className="home-msg-count">{conv.items.length}</span>}
                        <span className="home-msg-time">{latest.timeText}</span>
                      </div>
                      <div className="home-msg-snippet">{latest.body}</div>
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
        {chat ? (
          <>
            <div className="home-read-head">
              <span className="home-read-title">{chat.origin}</span>
              <span className="home-read-meta">{chatMsgs.length} 条消息</span>
            </div>
            <section className="section" style={{ margin: "5px 16px 16px", padding: "18px 22px", flex: 1, overflowY: "auto" }}>
              <div className="chat-flow">
                {chatMsgs.map((m) => {
                  const { sender, text } = splitSender(m.body);
                  return (
                    <div className="chat-row" key={`${m.received_at}-${m.origin}`}>
                      {sender && <span className="chat-sender">{sender}</span>}
                      <div className="chat-bubble">
                        <p className="chat-text">{text}</p>
                        <small className="chat-stime">{m.timeText}</small>
                        <button
                          className="chat-copy"
                          onClick={async (e) => { e.stopPropagation(); await navigator.clipboard.writeText(m.body); }}
                        >
                          复制
                        </button>
                      </div>
                    </div>
                  );
                })}
              </div>
              <div className="home-read-actions">
                {chat.app_id && (
                  <button className="imsg-copyall" onClick={async (e) => { e.stopPropagation(); await openAppById(chat.app_id!).catch(() => {}); }}>
                    打开应用
                  </button>
                )}
              </div>
            </section>
          </>
        ) : selected ? (
          <>
            <div className="home-read-head">
              <span className="home-read-title">{selected.origin}</span>
              <span className="home-read-meta">{selected.timeText}{selected.category === 1 ? ` · ${getCallStatusText(selected.call_outcome, selected.body)}` : ""}</span>
            </div>
            <section className="section" style={{ margin: "5px 16px 16px", padding: "18px 22px", flex: 1, overflowY: "auto" }}>
              {/* 来电参数面板 */}
              {selected.category === 1 && (
                <div className="call-meta">
                  {/* 通话状态徽章 */}
                  {selected.call_outcome && (
                    <div className={`call-status-badge ${selected.call_outcome}`}>
                      {selected.call_outcome === "answered" ? "✓ 已接通" : selected.call_outcome === "missed" ? "✗ 未接听" : ""}
                      {extractDurationSecs(selected.body) > 0 ? (
                        <span className="call-duration">持续 {extractDurationSecs(selected.body)} 秒</span>
                      ) : null}
                    </div>
                  )}
                  {selected.carrier && (
                    <div className="call-meta-group">
                      <span className="call-meta-label">运营商</span>
                      <span className="call-meta-value">{selected.carrier}</span>
                    </div>
                  )}
                  {selected.device_name && (
                    <div className="call-meta-group">
                      <span className="call-meta-label">手机名称</span>
                      <span className="call-meta-value">{selected.device_name}</span>
                    </div>
                  )}
                  {selected.uid != null && (
                    <div className="call-meta-group">
                      <span className="call-meta-label">ANCS UID</span>
                      <span className="call-meta-value">{selected.uid}</span>
                    </div>
                  )}
                  {selected.device_id && (
                    <div className="call-meta-group">
                      <span className="call-meta-label">设备 ID</span>
                      <span className="call-meta-value" style={{ fontSize: 11, fontFamily: "ui-monospace, monospace" }}>{selected.device_id}</span>
                    </div>
                  )}
                  {selected.title && selected.title !== selected.origin && (
                    <div className="call-meta-group">
                      <span className="call-meta-label">标题</span>
                      <span className="call-meta-value">{selected.title}</span>
                    </div>
                  )}
                  {selected.subtitle && (
                    <div className="call-meta-group">
                      <span className="call-meta-label">副标题</span>
                      <span className="call-meta-value">{selected.subtitle}</span>
                    </div>
                  )}
                  {selected.message && selected.message !== selected.body && (
                    <div className="call-meta-group" style={{ flex: 1 }}>
                      <span className="call-meta-label">正文</span>
                      <span className="call-meta-value" style={{ whiteSpace: "pre-wrap" }}>{selected.message}</span>
                    </div>
                  )}
                </div>
              )}
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
            </section>
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

function encodeKey(receivedAt: number, origin: string): string {
  return `${receivedAt}—${origin.replace(/[^a-zA-Z0-9]/g, "_")}`;
}

/** 来电状态文字：接通显示时长，未接显示"未接听" */
function getCallStatusText(outcome: string | null, body: string): string {
  if (outcome === "missed") return "未接听";
  if (outcome === "answered") {
    const secs = extractDurationSecs(body);
    return secs > 0 ? `已接通 · ${secs}秒` : "已接通";
  }
  return "来电";
}

/** 从通话记录正文提取持续秒数 */
function extractDurationSecs(body: string): number {
  const m = body.match(/(\d+)\s*秒/);
  return m ? parseInt(m[1], 10) : 0;
}

function brandAssets(entries: HistoryEntry[]): Record<string, string | null> {
  const map: Record<string, string | null> = {};
  entries.forEach((entry) => { if (entry.app_id) map[entry.app_id] = brandAsset(entry.app_id); });
  return map;
}
