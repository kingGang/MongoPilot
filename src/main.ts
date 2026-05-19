import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";

// Monaco Editor worker 配置（语法着色依赖 worker）
import("monaco-editor/esm/vs/editor/editor.worker?worker")
  .then((editorMod) => {
    return import("monaco-editor/esm/vs/language/typescript/ts.worker?worker").then((tsMod) => {
      self.MonacoEnvironment = {
        getWorker(_: unknown, label: string) {
          if (label === "typescript" || label === "javascript") {
            return new tsMod.default();
          }
          return new editorMod.default();
        },
      };
    });
  })
  .catch(() => {
    // worker 加载失败不影响基本功能
  });

/**
 * 启动期窗口隐藏 (tauri.conf.json: visible:false), Vue mount + 第一次绘制后再 show().
 * 解决 Win11 全新安装首次启动白屏:
 *   删了 %LOCALAPPDATA%\com.mongopilot.app\EBWebView\ 后, WebView2 要冷启动
 *   + 重建用户数据目录, 期间窗口已显示但内容空白 1~3 秒.
 *   把窗口隐到 UI 真正画完再露脸, 避免用户看到那段空白.
 *
 * 两道兜底, 都是防止"窗口永远隐藏 -> 用户只能从任务管理器杀进程":
 *   1. mount 用 try/catch 包住, 即使 Vue 装载失败也得露脸
 *   2. setTimeout 3s 后无脑 show, 防止 rAF 被 hidden-window 节流 / show() 抛错
 */
let _shown = false;
function revealWindow() {
  if (_shown) return;
  _shown = true;
  document.getElementById("app-splash")?.remove();
  import("@tauri-apps/api/window")
    .then((m) => {
      const win = m.getCurrentWindow();
      win.show().catch(() => { /* 非 tauri 或已显示 */ });
      win.setFocus().catch(() => { /* ignore */ });
    })
    .catch(() => { /* 浏览器预览模式 (pnpm dev), 忽略 */ });
}
// 兜底 #2: rAF 被节流 / show() 抛错时仍然能让窗口露脸 (3 秒内 UI 没渲完也强制显示)
const _safetyTimer = setTimeout(revealWindow, 3000);

try {
  const app = createApp(App);
  app.use(createPinia());
  app.mount("#app");
} catch (e) {
  // 兜底 #1: Vue 装载失败也露脸, 让用户能看到错误状态而不是无声隐藏
  // eslint-disable-next-line no-console
  console.error("Vue mount 失败:", e);
  clearTimeout(_safetyTimer);
  revealWindow();
}

// 正常路径: 两次 rAF -> 第一帧提交样式, 第二帧绘制完成 -> show()
requestAnimationFrame(() => {
  requestAnimationFrame(() => {
    clearTimeout(_safetyTimer);
    revealWindow();
  });
});
