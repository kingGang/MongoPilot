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

const app = createApp(App);
app.use(createPinia());
app.mount("#app");

// Vue mount 完成 -> 移除启动 splash (在 index.html 里), 避免首次启动一片空白
document.getElementById("app-splash")?.remove();
