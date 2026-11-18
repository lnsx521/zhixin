/** 常见 App → 内置官方图标资源（bundle id → 资源路径）。
 * 图标来自 macosicons.com，256px PNG；打包时随 dist 一起出。 */
export const BRAND_ASSETS: Record<string, string> = {
  "com.apple.mobilesms": "/brands/messages.png",
  "com.tencent.xin": "/brands/wechat.png",
  "com.tencent.wework": "/brands/wecom.png",
  "com.apple.mail": "/brands/mail.png",
  "com.apple.mobilemail": "/brands/mail.png",
};

/** 按 bundle id 找内置品牌图标；找不到返回 null（用联网缓存图标或兜底）。 */
export function brandAsset(appId: string | null): string | null {
  const key = (appId ?? "").toLowerCase();
  for (const [id, asset] of Object.entries(BRAND_ASSETS)) {
    if (key === id || key.endsWith(id)) return asset;
  }
  return null;
}
