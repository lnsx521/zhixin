import type { CSSProperties, ReactNode } from "react";

export type IconName = "phone" | "bell" | "code" | "settings" | "message" | "plus" | "chevron" | "info" | "star" | "external" | "lock" | "folder" | "app-sms" | "app-wechat" | "app-mail";

const paths: Record<IconName, ReactNode> = {
  phone: <><rect x="6" y="2.5" width="12" height="19" rx="2.5" /><path d="M10 5h4M11 18.5h2" /></>,
  bell: <><path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9ZM10 21h4" /></>,
  code: <><rect x="3" y="5" width="18" height="14" rx="2" /><path d="m8 10-2 2 2 2m8-4 2 2-2 2m-3-4-2 4" /></>,
  settings: <><path d="M4 6h16M4 12h16M4 18h16" /><path d="M8 4v4m8 2v4m-6 2v4" /></>,
  message: <path d="M5 3h14a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2h-8l-6 3v-3a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2Zm2 5h10M7 12h7" />,
  plus: <path d="M12 5v14M5 12h14" />,
  chevron: <path d="m9 5 7 7-7 7" />,
  info: <><circle cx="12" cy="12" r="9" /><path d="M12 11v6M12 7h.01" /></>,
  star: <path d="m12 3 2.8 5.7 6.3.9-4.6 4.5 1.1 6.3-5.6-3-5.6 3 1.1-6.3L3 9.6l6.2-.9Z" />,
  external: <><path d="M14 3h7v7M21 3l-9 9M10 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-5" /></>,
  lock: <><rect x="5" y="10.5" width="14" height="10" rx="2" /><path d="M8 10.5V7a4 4 0 0 1 8 0v3.5" /></>,
  folder: <path d="M3.5 7a2 2 0 0 1 2-2h4l2 2.5h7a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2Z" />,
  "app-sms": <path d="M12 4.2c-4.6 0-8.4 2.9-8.4 6.5 0 2 1.1 3.9 2.9 5.1l-.7 3 3.4-1.8c.9.2 1.8.3 2.8.3 4.6 0 8.4-2.9 8.4-6.6 0-3.6-3.8-6.5-8.4-6.5Z" fill="currentColor" stroke="none" />,
  "app-wechat": <><path d="M9.6 4.6C5.5 4.6 2.2 7.2 2.2 10.4c0 1.8 1 3.4 2.7 4.5l-.8 2.9 3.2-1.7c.7.2 1.5.3 2.3.3.3 0 .7 0 1-.1-.2-.6-.3-1.2-.3-1.9 0-3.2 3.1-5.8 6.9-5.8h.3c-.9-2.6-4-4-7.6-4Z" fill="currentColor" stroke="none" /><circle cx="6.2" cy="8.6" r="1.05" fill="var(--icon-gap, #0abf53)" stroke="none" /><circle cx="9.4" cy="8.6" r="1.05" fill="var(--icon-gap, #0abf53)" stroke="none" /><path d="M21.8 14.6c0-2.5-2.5-4.5-5.6-4.5s-5.6 2-5.6 4.5 2.5 4.5 5.6 4.5c.6 0 1.3-.1 1.8-.2l2.6 1.4-.7-2.5c1.2-.8 1.9-1.9 1.9-3.2Z" fill="currentColor" stroke="var(--icon-gap, #0abf53)" stroke-width="1.3" /><circle cx="14.8" cy="13.7" r="0.9" fill="var(--icon-gap, #0abf53)" stroke="none" /><circle cx="17.2" cy="13.7" r="0.9" fill="var(--icon-gap, #0abf53)" stroke="none" /></>,
  "app-mail": <><rect x="3.2" y="5.8" width="17.6" height="12.4" rx="2.4" fill="currentColor" stroke="none" /><path d="m4.6 8.2 7.4 5.1 7.4-5.1" fill="none" stroke="var(--icon-gap, #1e6ff1)" stroke-width="1.6" /></>,
};

export function Icon({ name, size = 18, style }: { name: IconName; size?: number; style?: CSSProperties }) {
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>{paths[name]}</svg>;
}
