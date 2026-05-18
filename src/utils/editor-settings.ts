/**
 * 编辑器外观设置 —— 全局共享, 持久化到 localStorage。
 * MonacoEditor 读取并应用; QueryToolbar 的设置弹层修改它。
 */
import { reactive, watch } from "vue";

export interface EditorSettings {
  /** 字号 px */
  fontSize: number;
  /** 自动换行 */
  wordWrap: boolean;
  /** 显示小地图 (minimap) */
  minimap: boolean;
}

const LS_KEY = "mongopilot.editorSettings";
const DEFAULTS: EditorSettings = { fontSize: 14, wordWrap: true, minimap: false };

function load(): EditorSettings {
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (raw) {
      const p = JSON.parse(raw) as Partial<EditorSettings>;
      return {
        fontSize: typeof p.fontSize === "number" ? p.fontSize : DEFAULTS.fontSize,
        wordWrap: typeof p.wordWrap === "boolean" ? p.wordWrap : DEFAULTS.wordWrap,
        minimap: typeof p.minimap === "boolean" ? p.minimap : DEFAULTS.minimap,
      };
    }
  } catch {
    /* ignore */
  }
  return { ...DEFAULTS };
}

export const editorSettings = reactive<EditorSettings>(load());

watch(
  editorSettings,
  (s) => {
    try {
      localStorage.setItem(LS_KEY, JSON.stringify(s));
    } catch {
      /* ignore */
    }
  },
  { deep: true },
);
