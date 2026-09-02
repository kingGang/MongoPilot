/**
 * 图片链接悬停预览 —— 字段值是图片 URL 时, 鼠标移上去弹出缩略图。
 *
 * 判定不靠后缀名硬猜: 先粗筛掉明显不是图片的链接, 剩下的真的 `new Image()` 加载一次,
 * 加载成功才弹浮层, 失败的结果缓存起来不再重复请求。这样没有后缀的 CDN 链接也能预览。
 */
import { ref, onBeforeUnmount } from "vue";

/** 常见图片后缀 (带 query/hash 也认) */
const IMAGE_EXT_RE = /\.(png|jpe?g|gif|webp|bmp|svg|ico|avif|tiff?|heics?)(\?|#|$)/i;
/** 明显不是图片的后缀 —— 不去无谓地发请求 */
const NON_IMAGE_EXT_RE =
  /\.(json|xml|html?|txt|md|csv|pdf|zip|gz|tar|rar|7z|mp4|mov|avi|mkv|webm|mp3|wav|flac|js|mjs|css|exe|msi|dmg|apk|ipa|docx?|xlsx?|pptx?)(\?|#|$)/i;

/**
 * 值是否可能是图片链接; 是则返回规范化后的 URL, 否则 null。
 * 只接受 http/https 和 data:image/。
 */
export function imageUrlCandidate(val: unknown): string | null {
  if (typeof val !== "string") return null;
  const s = val.trim();
  if (s.length < 8 || s.length > 4096) return null;
  if (/^data:image\//i.test(s)) return s;
  if (!/^https?:\/\//i.test(s)) return null;
  if (IMAGE_EXT_RE.test(s)) return s;
  if (NON_IMAGE_EXT_RE.test(s)) return null;
  // 没后缀的链接 (对象存储/CDN 常见): 交给 probeImage 实际加载一次判定
  return s;
}

export interface ImageSize {
  w: number;
  h: number;
}

/** url -> 尺寸 (null = 加载失败, 不是图片); 跨表格/翻页共享, 悬停第二次直接命中 */
const probeCache = new Map<string, ImageSize | null>();
const MAX_CACHE = 500;

/** 加载一次图片拿原始尺寸; 失败返回 null。结果缓存, 超时不写缓存 (下次还能再试) */
export function probeImage(url: string): Promise<ImageSize | null> {
  const hit = probeCache.get(url);
  if (hit !== undefined) return Promise.resolve(hit);

  return new Promise((resolve) => {
    const img = new Image();
    let settled = false;
    const finish = (size: ImageSize | null, cache: boolean) => {
      if (settled) return;
      settled = true;
      if (cache) {
        if (probeCache.size >= MAX_CACHE) probeCache.clear();
        probeCache.set(url, size);
      }
      resolve(size);
    };
    img.onload = () => finish({ w: img.naturalWidth, h: img.naturalHeight }, true);
    img.onerror = () => finish(null, true);
    // 8s 还没结果就先不弹, 但不缓存失败 —— 可能只是网络慢
    setTimeout(() => finish(null, false), 8000);
    img.src = url;
  });
}

export interface ImageHoverAnchor {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

/**
 * 单实例图片浮层状态。每个用到的视图 (TableView / TreeDocView) 各建一份,
 * 配合 `ImageHoverPreview.vue` 使用。
 */
export function useImageHover(delay = 260) {
  const imgVisible = ref(false);
  const imgUrl = ref("");
  const imgSize = ref<ImageSize>({ w: 0, h: 0 });
  const imgAnchor = ref<ImageHoverAnchor>({ left: 0, right: 0, top: 0, bottom: 0 });

  let showTimer: ReturnType<typeof setTimeout> | null = null;
  let hideTimer: ReturnType<typeof setTimeout> | null = null;
  /** 并发保护: 鼠标已经移走时, 迟到的 probe 结果不许弹出来 */
  let token = 0;

  function clearTimers() {
    if (showTimer) {
      clearTimeout(showTimer);
      showTimer = null;
    }
    if (hideTimer) {
      clearTimeout(hideTimer);
      hideTimer = null;
    }
  }

  function rectOf(el: HTMLElement | null, e: MouseEvent): ImageHoverAnchor {
    if (el && el.isConnected) {
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom };
    }
    return { left: e.clientX, right: e.clientX, top: e.clientY, bottom: e.clientY };
  }

  /** 悬停在单元格上: 值像图片链接就延迟加载, 加载成功才弹 */
  function showImageHover(e: MouseEvent, val: unknown) {
    const url = imageUrlCandidate(val);
    if (!url) return;
    const el = e.currentTarget as HTMLElement | null;
    const fallback = rectOf(el, e);
    clearTimers();
    const my = ++token;
    showTimer = setTimeout(async () => {
      const size = await probeImage(url);
      if (my !== token || !size) return;
      imgUrl.value = url;
      imgSize.value = size;
      imgAnchor.value = el && el.isConnected ? rectOf(el, e) : fallback;
      imgVisible.value = true;
    }, delay);
  }

  /** 移开: 留一点时间让鼠标能移进浮层 (点图/复制) */
  function hideImageHoverSoon() {
    token++;
    if (showTimer) {
      clearTimeout(showTimer);
      showTimer = null;
    }
    if (hideTimer) clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      imgVisible.value = false;
      hideTimer = null;
    }, 160);
  }

  function cancelImageHoverHide() {
    if (hideTimer) {
      clearTimeout(hideTimer);
      hideTimer = null;
    }
  }

  /** 立即关掉 (点击/滚动/打开弹窗时) */
  function hideImageHover() {
    token++;
    clearTimers();
    imgVisible.value = false;
  }

  // 表格滚动后浮层就对不上单元格了, 直接收掉 (捕获阶段, 内层滚动容器也能收到)
  function onScrollCapture() {
    if (imgVisible.value || showTimer) hideImageHover();
  }
  window.addEventListener("scroll", onScrollCapture, true);

  onBeforeUnmount(() => {
    clearTimers();
    window.removeEventListener("scroll", onScrollCapture, true);
  });

  return {
    imgVisible,
    imgUrl,
    imgSize,
    imgAnchor,
    showImageHover,
    hideImageHoverSoon,
    cancelImageHoverHide,
    hideImageHover,
  };
}
