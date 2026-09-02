<script setup lang="ts">
/**
 * 图片链接的悬停浮层: 缩略图 + 原始尺寸, 按 P / 点击图片在浏览器打开。
 * 单实例, 由 `useImageHover()` 驱动 (见 utils/image-preview.ts)。
 */
import { computed, watch, onBeforeUnmount } from "vue";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import type { ImageHoverAnchor, ImageSize } from "@/utils/image-preview";

const props = defineProps<{
  show: boolean;
  url: string;
  /** 图片原始尺寸 */
  size: ImageSize;
  /** 触发单元格的视口矩形 */
  anchor: ImageHoverAnchor;
}>();

const emit = defineEmits<{ enter: []; leave: [] }>();

/** 缩略图最大边长 */
const MAX_SIDE = 420;
const PAD = 8;
const FOOT_H = 22;

/** 按原始比例缩放到 MAX_SIDE 以内 (小图不放大) */
const box = computed(() => {
  const { w, h } = props.size;
  if (!w || !h) return { w: 240, h: 180 };
  const scale = Math.min(1, MAX_SIDE / w, MAX_SIDE / h);
  return { w: Math.max(40, Math.round(w * scale)), h: Math.max(40, Math.round(h * scale)) };
});

/** 贴在单元格右侧; 放不下就翻到左边 / 上移, 始终留 8px 边距 */
const style = computed(() => {
  const gap = 6;
  const w = box.value.w + PAD * 2;
  const hTotal = box.value.h + FOOT_H + PAD * 2;
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const a = props.anchor;

  let left = a.right + gap;
  if (left + w > vw - 8) left = a.left - w - gap;
  if (left < 8) left = Math.max(8, Math.min(a.left, vw - w - 8));

  let top = a.top;
  if (top + hTotal > vh - 8) top = Math.max(8, vh - hTotal - 8);

  return { left: `${left}px`, top: `${top}px`, width: `${w}px` };
});

async function openInBrowser() {
  if (!props.url) return;
  try {
    await openUrl(props.url);
  } catch {
    /* 打不开就算了, 不打断浏览 */
  }
}

function onKeydown(e: KeyboardEvent) {
  if (!props.show) return;
  const t = e.target as HTMLElement | null;
  // 正在输入框里打字时不抢按键
  if (t && (/^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName) || t.isContentEditable)) return;
  if (e.key === "p" || e.key === "P") {
    e.preventDefault();
    openInBrowser();
  }
}

watch(
  () => props.show,
  (show) => {
    if (show) window.addEventListener("keydown", onKeydown);
    else window.removeEventListener("keydown", onKeydown);
  },
);
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <Teleport to="body">
    <div
      v-if="show"
      class="img-hover-float"
      :style="style"
      @mouseenter="emit('enter')"
      @mouseleave="emit('leave')"
    >
      <img
        :src="url"
        class="img-hover-img"
        :style="{ width: `${box.w}px`, height: `${box.h}px` }"
        alt=""
        title="点击在浏览器打开"
        @click="openInBrowser"
      />
      <div class="img-hover-foot">
        <span class="ihf-size">Size: {{ size.w }} × {{ size.h }}</span>
        <span class="ihf-hint">按 P 打开</span>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.img-hover-float {
  position: fixed;
  z-index: 3100;
  background: #fff;
  border: 1px solid #d0d0d0;
  border-radius: 4px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.18);
  padding: 8px;
  box-sizing: border-box;
}
.img-hover-img {
  display: block;
  object-fit: contain;
  cursor: pointer;
  background: #fafafa;
}
.img-hover-foot {
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  font-family: "Consolas", "Monaco", monospace;
  font-size: 12px;
  color: #666;
}
.ihf-hint {
  color: #999;
}
</style>
