import { useEffect, useMemo, useState } from "react";
import { clearNotificationHistory, getAppIcon, getCallStats, getLinkState, listNotificationHistory, openAppById, openProjectLink, type CallStats, type HistoryEntry, type ProjectLink } from "../../shared/api";
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

/** 电话类：category=1 或来源是系统电话（部分安卓 ANCS 兼容层用自定义类别，实测华为用 12） */
function isCallEntry(e: HistoryEntry): boolean {
  return e.category === 1 || /mobilephone/i.test(e.app_id ?? "");
}

function convKeyOf(e: HistoryEntry): string {
  return `${e.app_id ?? ""}|${e.origin}`;
}

/** 开发者的一封信（测试邮件）：正文中带项目主页/Star，阅读区渲染成可点卡片 */
function isDeveloperLetter(e: HistoryEntry): boolean {
  return e.app_id === "com.apple.mail" && /很高兴遇见你/.test(e.body);
}

/** 开发者来信卡片：按书信体排版（称呼→正文→落款右对齐）+ 维护者主页/Star 徽章可点 */
function DeveloperLetterCard({ entry }: { entry: HistoryEntry }) {
  const [badgeFailed, setBadgeFailed] = useState(false);
  const open = (target: ProjectLink) => { void openProjectLink(target).catch(() => {}); };
  const paragraphs = entry.body.split(/\n{2,}/).filter(Boolean);
  const last = paragraphs[paragraphs.length - 1];
  // 落款「—— lnsx521」从正文拆出：去掉破折号，署名 + 日期靠右分行，符合书信格式
  const signOff = last && /^——\s*/.test(last) ? paragraphs.pop()!.replace(/^——\s*/, "") : null;
  return (
    <div className="developer-letter-card">
      <div className="developer-letter-body">
        {paragraphs.map((p, i) => (
          <p key={i} className={i === 0 ? "developer-letter-salutation" : undefined}>{p}</p>
        ))}
        {signOff && (
          <div className="developer-letter-signoff">
            <span>{signOff}</span>
            <span>2026 年 11 月 21 日</span>
          </div>
        )}
      </div>
      <div className="developer-letter-foot">
        <span className="developer-letter-maintainer" role="link" tabIndex={0}
          onClick={() => open("maintainer")} onKeyDown={(e) => { if (e.key === "Enter") open("maintainer"); }}>
          维护者：<b>lnsx521</b> <Icon name="external" size={12} />
        </span>
        <span className="developer-letter-hint"><Icon name="star" size={14} />喜欢的话，去 GitHub 点个 Star</span>
        <button className="developer-letter-stars" aria-label="在 GitHub 上给知信点 Star" onClick={() => open("repository")}>
          {badgeFailed
            ? <span className="about-badge-fallback">Star on GitHub</span>
            : <img className="about-badge" src="https://img.shields.io/github/stars/lnsx521/zhixin?style=flat-square&label=Stars&color=737373" alt="Stars" referrerPolicy="no-referrer" onError={() => setBadgeFailed(true)} />}
        </button>
      </div>
    </div>
  );
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
  const [callStats, setCallStats] = useState<CallStats | null>(null);
  // 未读跟踪：会话 key → 上次打开时的条数；条数超过它才显示徽章
  const [readConvs, setReadConvs] = useState<Record<string, number>>({});
  const [confirmClear, setConfirmClear] = useState(false);

  const openSingle = (it: Item) => {
    setChat(null);
    setSelected(it);
  };

  // 选中电话时拉同号码历史统计（来电次数 / 首次来电）；同时接收历史变更刷新
  useEffect(() => {
    if (!selected || !isCallEntry(selected)) { setCallStats(null); return; }
    void getCallStats(selected.origin).then(setCallStats).catch(() => setCallStats(null));
  }, [selected?.origin, selected?.received_at, entries]);

  const openConv = (conv: Conv) => {
    setSelected(null);
    setChat(conv);
    setChatMsgs(conv.items);
    // 打开即标记已读：徽章基线 = 当前条数，再来新消息才重新亮
    setReadConvs((prev) => ({ ...prev, [conv.key]: conv.items.length }));
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
    // 新通知入库 / 清空历史时后端广播，实时刷新列表（含微信会话合并）
    const unlistenHistory = listen("history-updated", () => {
      void listNotificationHistory("", 50).then((rows) => setEntries(rows)).catch(() => {});
    });
    return () => {
      window.clearInterval(timer);
      unlistenHistory.then((f) => f());
    };
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
          {groups.map((g, gi) => (
            <div key={g.day}>
              <div className="home-day-row">
                <span className="home-day">{g.day}</span>
                {/* 只在第一组（最新日期）标题行显示清空按钮，避免每组重复 */}
                {gi === 0 && g.rows.length > 0 && (
                  <button
                    className="home-clear-pill"
                    title="清空全部历史消息"
                    onClick={(e) => { e.stopPropagation(); setConfirmClear(true); }}
                  >清空消息</button>
                )}
              </div>
              {g.rows.map((row) => {
                if (row.kind === "single") {
                  const it = row.item;
                  // 同一毫秒+同号码的重复旧数据曾导致 key 撞车（选中两条连蓝），
                  // 拼上通话结果保证唯一；选中判断同步防撞
                  const key = `${it.received_at}-${it.origin}-${it.call_outcome ?? ""}`;
                  const asset = brandAsset(it.app_id);
                  const isSel = selected?.received_at === it.received_at && selected.origin === it.origin
                    && (selected.call_outcome ?? "") === (it.call_outcome ?? "") && !chat;
                  return (
                    <div
                      key={key}
                      data-entry-key={encodeKey(it.received_at, it.origin)}
                      className={`home-msg ${isSel ? "selected" : ""}`}
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
                        {conv.items.length - (readConvs[conv.key] ?? 0) > 0 && (
                          <span className="home-msg-count">{conv.items.length - (readConvs[conv.key] ?? 0)}</span>
                        )}
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
              <span className="home-read-meta">{selected.timeText}{isCallEntry(selected) ? ` · ${getCallStatusText(selected.call_outcome, selected.body)}` : ""}</span>
            </div>
            <section className="section" style={{ margin: "5px 16px 16px", padding: "18px 22px", flex: 1, overflowY: "auto" }}>
              {/* 来电参数面板 */}
              {isCallEntry(selected) && (
                <div className="call-meta">
                  {/* 状态小药丸：固定在容器右下角给视觉焦点 */}
                  {selected.call_outcome && (
                    <span className={`call-status-pill ${selected.call_outcome}`}>
                      {getCallStatusText(selected.call_outcome, selected.body)}
                    </span>
                  )}
                  {/* 来电人（联系人名 / 号码，ANCS 只给显示名不给真实号） */}
                  <div className="call-meta-group">
                    <span className="call-meta-label">来电人</span>
                    <span className="call-meta-value">{selected.title || selected.origin}</span>
                  </div>
                  {/* 来电时间（完整） */}
                  <div className="call-meta-group">
                    <span className="call-meta-label">来电时间</span>
                    <span className="call-meta-value">{fmtFullTime(selected.received_at)}</span>
                  </div>
                  {/* 同号码历史来电统计 */}
                  {callStats && callStats.count > 0 && (
                    <div className="call-meta-group">
                      <span className="call-meta-label">来电次数</span>
                      <span className="call-meta-value">{callStats.count} 次</span>
                    </div>
                  )}
                  {/* 运营商：联系人号码 ANCS 不给真实号，识别不出时明确标注而非空着 */}
                  <div className="call-meta-group">
                    <span className="call-meta-label">运营商</span>
                    <span className="call-meta-value">{selected.carrier || "未知（联系人号码）"}</span>
                  </div>
                  {/* 号码标签（iOS 通话通知自带的副标题） */}
                  {selected.subtitle && (
                    <div className="call-meta-group">
                      <span className="call-meta-label">号码标签</span>
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
              {/* 电话的正文（"已接通 N秒"）已在上方药丸/参数面板体现，不重复显示气泡 */}
              {!isCallEntry(selected) && (
                isDeveloperLetter(selected)
                  ? <DeveloperLetterCard entry={selected} />
                  : <div className="imsg-bubble">
                      <p className="imsg-text">{selected.body}</p>
                      <small className="imsg-time">{selected.timeText}</small>
                    </div>
              )}
              <div className="home-read-actions">
                {selected.code && (
                  <button
                    className="toast-code"
                    onClick={async (e) => { e.stopPropagation(); await navigator.clipboard.writeText(selected.code!); }}
                  >
                    点击复制 {selected.code}
                  </button>
                )}
                {/* 电话详情无正文可复制（时长已在药丸），不显示复制按钮 */}
                {!isCallEntry(selected) && (
                  <button className="imsg-copyall" onClick={async (e) => { e.stopPropagation(); await navigator.clipboard.writeText(selected.body); }}>
                    复制全文
                  </button>
                )}
                {/* 电话/短信/开发者来信没有对应的电脑应用可唤起，不显示「打开应用」 */}
                {!isCallEntry(selected) && !isDeveloperLetter(selected) && !/mobilesms/i.test(selected.app_id ?? "") && selected.app_id && (
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
      {confirmClear && (
        <div className="confirm-overlay" onClick={() => setConfirmClear(false)}>
          <div className="confirm-box" onClick={(e) => e.stopPropagation()}>
            <div className="confirm-title">清空历史消息</div>
            <p className="confirm-desc">确定清空全部历史消息？此操作不可恢复。</p>
            <div className="confirm-actions">
              <button className="btn" onClick={() => setConfirmClear(false)}>取消</button>
              <button className="btn danger" onClick={async () => {
                setConfirmClear(false);
                try {
                  await clearNotificationHistory();
                  setEntries([]);
                  setSelected(null);
                  setChat(null);
                  setReadConvs({});
                } catch { /* 后端广播会刷新列表 */ }
              }}>清空</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function fmtTime(receivedAt: number): string {
  const d = new Date(receivedAt);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/** 完整时间：2026-11-21 21:19:32 */
function fmtFullTime(ms: number): string {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

function encodeKey(receivedAt: number, origin: string): string {
  return `${receivedAt}—${origin.replace(/[^a-zA-Z0-9]/g, "_")}`;
}

/** 来电状态文字：接通显示时长，未接/拒接各自显示 */
function getCallStatusText(outcome: string | null, body: string): string {
  if (outcome === "missed") return "未接听";
  if (outcome === "rejected") return "已拒接";
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
