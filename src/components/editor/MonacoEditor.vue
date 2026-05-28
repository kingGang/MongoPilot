<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, watch, nextTick } from "vue";
import * as monaco from "monaco-editor";
// 显式导入语言支持（防止 Vite/esbuild tree-shaking 丢弃 side-effect imports）
import "monaco-editor/esm/vs/basic-languages/javascript/javascript.contribution";
import "monaco-editor/esm/vs/language/typescript/monaco.contribution";
import { registerMongoCompletions } from "@/utils/mongo-completions";
import type { FieldCompletionInfo } from "@/utils/mongo-completions";
import {
  registerMongoLanguage,
  registerMongoTheme,
  MONGO_LANGUAGE_ID,
  MONGO_THEME_LIGHT,
} from "@/utils/mongo-language";
import { useEditorStore } from "@/stores/editor";
import { useDatabaseStore } from "@/stores/database";
import * as aiApi from "@/api/ai";
import { formatMongoShell } from "@/utils/mongo-format";
import { editorSettings } from "@/utils/editor-settings";

const props = defineProps<{
  modelValue: string;
  language?: string;
  /** 每次递增时触发执行当前语句 */
  runTrigger?: number;
  /** 每次递增时触发格式化 (工具栏 Format 按钮) */
  formatTrigger?: number;
  /** 所属编辑器 tab id —— AI 提议编辑的 diff 确认需要它 */
  tabId?: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: string];
  run: [statement: string];
  /** 运行编辑器里所有语句 (Ctrl+Shift+Enter / 工具栏 Run All) */
  runAll: [];
  /** 右键 "AI 分析选中代码" —— 选区已写入 editorStore, 父组件打开 AI 面板并触发 agent */
  aiAnalyze: [];
}>();

const editorRef = ref<HTMLDivElement>();
let editor: monaco.editor.IStandaloneCodeEditor | null = null;
let statementDecorations: monaco.editor.IEditorDecorationsCollection | null = null;

const editorStore = useEditorStore();

// ---- AI 提议编辑: diff 确认 ----
const diffRef = ref<HTMLDivElement>();
let diffEditor: monaco.editor.IStandaloneDiffEditor | null = null;
let diffModels: { original: monaco.editor.ITextModel; modified: monaco.editor.ITextModel } | null =
  null;

/** 当前 tab 是否有 AI 提议的、未确认的编辑 */
const pendingContent = computed(() =>
  props.tabId ? editorStore.pendingEdits[props.tabId] : undefined,
);
const hasPendingEdit = computed(() => pendingContent.value !== undefined);

/** 建 / 更新 diff 编辑器: 左=当前内容, 右=AI 提议内容 (只读) */
function setupDiff(modified: string) {
  if (!diffRef.value) return;
  if (!diffEditor) {
    diffEditor = monaco.editor.createDiffEditor(diffRef.value, {
      readOnly: true,
      automaticLayout: true,
      minimap: { enabled: false },
      fontSize: 14,
      renderSideBySide: true,
      theme: MONGO_THEME_LIGHT,
      scrollBeyondLastLine: false,
    });
  }
  const old = diffModels;
  diffModels = {
    original: monaco.editor.createModel(props.modelValue, MONGO_LANGUAGE_ID),
    modified: monaco.editor.createModel(modified, MONGO_LANGUAGE_ID),
  };
  diffEditor.setModel(diffModels);
  if (old) {
    old.original.dispose();
    old.modified.dispose();
  }
}

function onAcceptEdit() {
  if (props.tabId) editorStore.acceptEdit(props.tabId);
}
function onRejectEdit() {
  if (props.tabId) editorStore.rejectEdit(props.tabId);
}

/** 把编辑器当前选区文本同步进 editorStore, 供 AI 工具 get_editor_selection 读取 */
function syncSelection() {
  if (!editor || !props.tabId) return;
  const sel = editor.getSelection();
  const text = sel && !sel.isEmpty() ? editor.getModel()?.getValueInRange(sel) ?? "" : "";
  editorStore.setSelection(props.tabId, text);
}

watch(pendingContent, async (val) => {
  if (val === undefined) return;
  await nextTick();
  setupDiff(val);
});
const dbStore = useDatabaseStore();

// ---- Schema 缓存 ----
const schemaCache = new Map<string, { fields: FieldCompletionInfo[]; ts: number }>();
const CACHE_TTL = 5 * 60 * 1000; // 5 分钟

async function getFieldNames(collection: string): Promise<FieldCompletionInfo[]> {
  const tab = editorStore.activeTab;
  if (!tab) return [];

  const cacheKey = `${tab.connectionId}:${tab.database}:${collection}`;
  const cached = schemaCache.get(cacheKey);
  if (cached && Date.now() - cached.ts < CACHE_TTL) {
    return cached.fields;
  }

  try {
    const schema = await aiApi.analyzeSchema(tab.connectionId, tab.database, collection, 50);
    const fields: FieldCompletionInfo[] = schema.fields.map((f) => ({
      name: f.name,
      types: f.fieldTypes.map((t) => t.bsonType).join(", "),
      collection,
    }));
    schemaCache.set(cacheKey, { fields, ts: Date.now() });
    return fields;
  } catch {
    return [];
  }
}

// 关闭 Monaco 内置 JS 语法检查（与 MongoDB Shell 语法不兼容）
monaco.languages.typescript.javascriptDefaults.setDiagnosticsOptions({
  noSemanticValidation: true,
  noSyntaxValidation: true,
});

// 注册自定义 mongosh 语言 (Monarch tokenizer + brackets config) 和配套主题
registerMongoLanguage();
registerMongoTheme();

// 注册 MongoDB 自动补全（全局只注册一次；对 JS 和 mongosh 两个 language id 都挂）
registerMongoCompletions({
  collectionNames: () => {
    const tab = editorStore.activeTab;
    if (!tab) return [];
    const colls = dbStore.getCollections(tab.connectionId, tab.database);
    return colls.map((c) => c.name);
  },
  getFieldNames,
  currentCollection: () => editorStore.activeTab?.collection || "",
});

// ---- 语句解析 ----

interface StatementRange {
  startLine: number;
  endLine: number;
  text: string;
}

/** 标记哪些行是注释（包括多行块注释） */
function buildCommentSet(lines: string[]): Set<number> {
  const set = new Set<number>();
  let inBlock = false;
  for (let i = 0; i < lines.length; i++) {
    const trimmed = lines[i].trim();
    if (inBlock) {
      set.add(i);
      if (trimmed.includes("*/")) inBlock = false;
    } else if (trimmed.startsWith("//")) {
      set.add(i);
    } else if (trimmed.startsWith("/*")) {
      set.add(i);
      if (!trimmed.includes("*/")) inBlock = true;
    }
  }
  return set;
}

/** 单行的 ( { [ - ) } ] 净变化, 跳过字符串和行内 // 块/行注释. */
function bracketDelta(line: string): number {
  let d = 0;
  let inStr = false;
  let strCh = "";
  let i = 0;
  const n = line.length;
  while (i < n) {
    const ch = line[i];
    if (inStr) {
      if (ch === "\\" && i + 1 < n) { i += 2; continue; }
      if (ch === strCh) inStr = false;
      i++;
      continue;
    }
    if (ch === '"' || ch === "'") { inStr = true; strCh = ch; i++; continue; }
    // 行注释截断: 后面的全部忽略
    if (ch === "/" && line[i + 1] === "/") break;
    // 行内块注释 /* ... */
    if (ch === "/" && line[i + 1] === "*") {
      i += 2;
      while (i + 1 < n && !(line[i] === "*" && line[i + 1] === "/")) i++;
      i = Math.min(i + 2, n);
      continue;
    }
    if (ch === "(" || ch === "{" || ch === "[") d++;
    else if (ch === ")" || ch === "}" || ch === "]") d--;
    i++;
  }
  return d;
}

/** 检测 top-level JS 代码块 (function / const / let / var / class / if / for / while / import / export):
 *  返回这些块所覆盖的编辑器行号集合 (1-indexed), lint 不再报"无法识别"
 */
function buildJsCodeBlocks(lines: string[], commentLines: Set<number>): Set<number> {
  const covered = new Set<number>();
  let inBlock = false;
  let depth = 0;
  const starterRe =
    /^(async\s+)?function\b|^const\b|^let\b|^var\b|^class\b|^export\b|^import\b|^if\s*\(|^for\s*\(|^while\s*\(|^try\b|^switch\s*\(|^[a-zA-Z_$][\w$]*\s*\(/;
  for (let i = 0; i < lines.length; i++) {
    if (commentLines.has(i)) {
      if (inBlock) covered.add(i + 1);
      continue;
    }
    const trimmed = lines[i].trim();
    if (!inBlock) {
      if (!trimmed) continue;
      if (
        trimmed.startsWith("db.") ||
        trimmed.startsWith("use ") ||
        trimmed.startsWith("show ")
      ) {
        continue;
      }
      if (starterRe.test(trimmed)) {
        inBlock = true;
        depth = bracketDelta(lines[i]);
        covered.add(i + 1);
        if (depth <= 0) inBlock = false; // 单行声明: const foo = 1;
      }
    } else {
      covered.add(i + 1);
      depth += bracketDelta(lines[i]);
      if (depth <= 0) inBlock = false;
    }
  }
  return covered;
}

function parseStatements(content: string): StatementRange[] {
  const lines = content.split("\n");
  const commentLines = buildCommentSet(lines);
  const statements: StatementRange[] = [];
  let current: { startLine: number; lines: string[] } | null = null;
  /** 当前累积语句的括号净深度. > 0 表示语句未闭合 */
  let depth = 0;

  const flush = () => {
    if (!current) return;
    statements.push({
      startLine: current.startLine,
      endLine: current.startLine + current.lines.length - 1,
      text: current.lines.join("\n"),
    });
    current = null;
    depth = 0;
  };

  for (let i = 0; i < lines.length; i++) {
    // 注释行
    if (commentLines.has(i)) {
      if (current && depth > 0) {
        // 语句未闭合, 注释当作语句体的一部分保持行号对齐, 不参与括号计算
        current.lines.push(lines[i]);
      } else if (current) {
        flush();
      }
      continue;
    }

    const line = lines[i];
    const trimmed = line.trim();

    if (
      trimmed.startsWith("db.") ||
      trimmed.startsWith("use ") ||
      trimmed.startsWith("show ")
    ) {
      // 新语句开头之前先收尾旧的
      flush();
      current = { startLine: i + 1, lines: [line] };
      depth = bracketDelta(line);
    } else if (current && trimmed.startsWith(".")) {
      // 链式调用续行 (例如格式化后的 .projection({}) 单独成行, 即便上一行已闭合也算续行)
      current.lines.push(line);
      depth += bracketDelta(line);
    } else if (current && (trimmed !== "" || depth > 0)) {
      // 续行: 非空, 或语句未闭合 (允许语句中间出现空行)
      current.lines.push(line);
      depth += bracketDelta(line);
    } else if (current && trimmed === "" && depth === 0) {
      // 空行: 若下一非空非注释行是 .xxx (链式), 视为续行, 不 flush
      let nextNonEmpty = -1;
      for (let k = i + 1; k < lines.length; k++) {
        if (commentLines.has(k)) continue;
        if (lines[k].trim()) { nextNonEmpty = k; break; }
      }
      if (nextNonEmpty >= 0 && lines[nextNonEmpty].trim().startsWith(".")) {
        current.lines.push(line);
      } else {
        flush();
      }
    }
    // !current && trimmed === "" → 直接忽略
  }

  flush();
  return statements;
}

// ---- 运行目标追踪 ----
// 始终缓存"当前应执行的语句"，按钮点击导致的失焦不会覆盖它
let _runTarget = "";
let _selClearTimer: ReturnType<typeof setTimeout> | null = null;

/** 根据光标位置计算应执行的语句（无选区时） */
function getStatementByCursor(): string {
  if (!editor) return props.modelValue;
  const position = editor.getPosition();
  if (!position) return props.modelValue;

  const content = editor.getValue();
  const statements = parseStatements(content);

  for (const stmt of statements) {
    if (position.lineNumber >= stmt.startLine && position.lineNumber <= stmt.endLine) {
      return stmt.text;
    }
  }

  if (statements.length === 1) return statements[0].text;
  if (statements.length === 0) return content;

  let closest = statements[0];
  let minDist = Infinity;
  for (const stmt of statements) {
    const dist = Math.min(
      Math.abs(position.lineNumber - stmt.startLine),
      Math.abs(position.lineNumber - stmt.endLine),
    );
    if (dist < minDist) { minDist = dist; closest = stmt; }
  }
  return closest.text;
}

/** 选区/光标变化时更新 _runTarget */
function updateRunTarget() {
  if (!editor) return;
  const sel = editor.getSelection();

  if (sel && !sel.isEmpty()) {
    // 有选区 → 立即更新
    const text = editor.getModel()?.getValueInRange(sel) ?? "";
    if (text.trim()) {
      if (_selClearTimer) { clearTimeout(_selClearTimer); _selClearTimer = null; }
      _runTarget = text;
      return;
    }
  }

  // 选区变空（可能是按钮点击导致失焦）→ 延迟 300ms 再覆盖
  // 让 handleRun 有机会先用旧值
  if (_selClearTimer) clearTimeout(_selClearTimer);
  _selClearTimer = setTimeout(() => {
    _runTarget = getStatementByCursor();
    _selClearTimer = null;
  }, 300);
}

// 监听 runTrigger 变化 → 用缓存的 _runTarget 执行
watch(() => props.runTrigger, () => {
  handleRun();
}, { flush: "sync" });

function updateStatementHighlight() {
  if (!editor) return;

  const position = editor.getPosition();
  if (!position) return;

  const content = editor.getValue();
  const statements = parseStatements(content);

  if (statements.length <= 1) {
    statementDecorations?.clear();
    return;
  }

  let activeStmt: StatementRange | null = null;
  for (const stmt of statements) {
    if (position.lineNumber >= stmt.startLine && position.lineNumber <= stmt.endLine) {
      activeStmt = stmt;
      break;
    }
  }

  if (!activeStmt) {
    statementDecorations?.clear();
    return;
  }

  const decorations: monaco.editor.IModelDeltaDecoration[] = [{
    range: new monaco.Range(activeStmt.startLine, 1, activeStmt.endLine, 1),
    options: {
      isWholeLine: true,
      className: "current-statement-highlight",
      overviewRuler: {
        color: "#3875d755",
        position: monaco.editor.OverviewRulerLane.Full,
      },
    },
  }];

  if (statementDecorations) {
    statementDecorations.clear();
    statementDecorations.set(decorations);
  }
}

// ---- 语法检查 ----

let lintTimer: ReturnType<typeof setTimeout> | null = null;

/**
 * 将 MongoDB Shell 宽松 JSON (unquoted keys) 转为标准 JSON 以便 JSON.parse 验证。
 * 例: {_id:1, name:"test"} → {"_id":1, "name":"test"}
 *     ObjectId("abc")      → "ObjectId(abc)"
 */
/**
 * 把所有 `identifier(...)` 调用 (含 `new Foo(...)`) 替换成 JSON 安全的占位字符串.
 * 字符串感知: 不动字符串内部的同样写法; 括号配对感知: 支持嵌套.
 * 覆盖范围: ObjectId/ISODate 等 shell 构造器 + 用户自定义 helper 函数 (encryptPhoneNumber(...) 之类).
 */
function replaceFunctionCalls(text: string): string {
  let out = "";
  const n = text.length;
  let i = 0;
  const isIdentStart = (c: string) => /[a-zA-Z_$]/.test(c);
  const isIdentChar = (c: string) => /[a-zA-Z0-9_$]/.test(c);

  while (i < n) {
    const c = text[i];
    // 跳过字符串
    if (c === '"' || c === "'") {
      const q = c;
      out += c;
      i++;
      while (i < n) {
        out += text[i];
        if (text[i] === "\\" && i + 1 < n) {
          out += text[i + 1];
          i += 2;
          continue;
        }
        if (text[i] === q) {
          i++;
          break;
        }
        i++;
      }
      continue;
    }
    if (isIdentStart(c)) {
      const callStart = i;
      let j = i;
      while (j < n && isIdentChar(text[j])) j++;
      let ident = text.slice(i, j);
      // new Foo(...) : 把 new + 构造器名一并算进 callStart..
      if (ident === "new") {
        let p = j;
        while (p < n && /\s/.test(text[p])) p++;
        if (p < n && isIdentStart(text[p])) {
          let q = p;
          while (q < n && isIdentChar(text[q])) q++;
          j = q;
          ident = text.slice(p, q);
        }
      }
      // 跳过空白看是否跟 (
      let k = j;
      while (k < n && /\s/.test(text[k])) k++;
      if (k < n && text[k] === "(") {
        // 配对找到对应 )
        let depth = 0;
        let m = k;
        let inStr = false;
        let strCh = "";
        for (; m < n; m++) {
          const ch = text[m];
          if (inStr) {
            if (ch === "\\") { m++; continue; }
            if (ch === strCh) inStr = false;
            continue;
          }
          if (ch === '"' || ch === "'") { inStr = true; strCh = ch; continue; }
          if (ch === "(") depth++;
          else if (ch === ")") {
            depth--;
            if (depth === 0) break;
          }
        }
        if (m < n && depth === 0) {
          out += JSON.stringify(text.slice(callStart, m + 1));
          i = m + 1;
          continue;
        }
      }
      // 不是函数调用, 原样输出标识符
      out += text.slice(callStart, j);
      i = j;
      continue;
    }
    out += c;
    i++;
  }
  return out;
}

function relaxJsonForValidation(text: string): string {
  // 0. 剥掉 JS 风格注释 (//... 与 /* ... */); 字符串内的同样写法保留.
  let result = stripJsComments(text);
  // 1. 把所有 identifier(...) 调用替换成占位字符串.
  //    覆盖 ObjectId/ISODate 等 shell 类型, 也覆盖用户自定义 helper 函数调用.
  result = replaceFunctionCalls(result);
  // 正则字面量 /pattern/flags
  result = result.replace(/\/(?:[^/\\]|\\.)+\/[gimsuy]*/g, (m) => JSON.stringify(m));

  // 2. 给未加引号的 key 加引号
  result = result.replace(
    /([{,]\s*)([a-zA-Z_$][a-zA-Z0-9_$]*)(\s*:)/g,
    '$1"$2"$3',
  );
  // 3. 把"裸标识符值" (脚本里引用的 var / 形参, 如 player._id / encryptedNewPhone)
  //    也替成占位字符串. 这步在 key 加引号之后做, 避免错把刚加的 key 再吃一遍.
  //    lookbehind: 前面不是 " ' 字母 数字 _ $ . —— 排除已在引号里 / 已是标识符一部分.
  //    lookahead: 后面不接 ( —— 函数调用已经在第 1 步处理过.
  result = result.replace(
    /(?<!["'\w$.])[a-zA-Z_$][\w$]*(?:\.[a-zA-Z_$][\w$]*)*(?!\s*\()/g,
    (m) => {
      if (/^(true|false|null|undefined|NaN|Infinity)$/.test(m)) return m;
      return JSON.stringify(m);
    },
  );
  // 4. 去除尾随逗号 (Shell 风格 mongo 容忍 {a:1,} / [1,2,] / 多 stage 管道 stage,)
  //    JSON.parse 不容尾随逗号, 会让 $project 等后续 stage 被误标红.
  result = result.replace(/,(\s*[}\]])/g, "$1");
  return result;
}

/** 剥掉 JS 风格 //... 行注释 与 /* *\/ 块注释; 字符串内的不动. */
function stripJsComments(text: string): string {
  let out = "";
  const n = text.length;
  let i = 0;
  while (i < n) {
    const c = text[i];
    if (c === '"' || c === "'") {
      const quote = c;
      out += c;
      i++;
      while (i < n) {
        const sc = text[i];
        out += sc;
        i++;
        if (sc === "\\" && i < n) {
          out += text[i];
          i++;
        } else if (sc === quote) {
          break;
        }
      }
      continue;
    }
    if (c === "/" && i + 1 < n && text[i + 1] === "/") {
      i += 2;
      while (i < n && text[i] !== "\n") i++;
      continue;
    }
    if (c === "/" && i + 1 < n && text[i + 1] === "*") {
      i += 2;
      while (i + 1 < n && !(text[i] === "*" && text[i + 1] === "/")) i++;
      i = Math.min(i + 2, n);
      continue;
    }
    out += c;
    i++;
  }
  return out;
}

/**
 * 从语句中提取所有方法调用的参数文本（括号内的内容）。
 * 返回 [{text, startLine, startCol}]
 */
function extractMethodArgs(
  stmtText: string,
  stmtStartLine: number,
): { argText: string; line: number; col: number }[] {
  const results: { argText: string; line: number; col: number }[] = [];
  const lines = stmtText.split("\n");

  // 找方法名后的 ( 及其匹配的 )
  let inString = false;
  let strChar = "";
  let parenStart: { lineIdx: number; colIdx: number } | null = null;
  let depth = 0;

  for (let li = 0; li < lines.length; li++) {
    const line = lines[li];
    for (let ci = 0; ci < line.length; ci++) {
      const ch = line[ci];
      if (inString) {
        if (ch === strChar && line[ci - 1] !== "\\") inString = false;
        continue;
      }
      if (ch === '"' || ch === "'") { inString = true; strChar = ch; continue; }
      if (ch === "(") {
        depth++;
        if (depth === 1) {
          parenStart = { lineIdx: li, colIdx: ci };
        }
      } else if (ch === ")") {
        depth--;
        if (depth === 0 && parenStart) {
          // 提取括号内的文本
          const argText = extractRange(lines, parenStart.lineIdx, parenStart.colIdx + 1, li, ci);
          if (argText.trim()) {
            results.push({
              argText: argText.trim(),
              line: stmtStartLine + parenStart.lineIdx,
              col: parenStart.colIdx + 2,
            });
          }
          parenStart = null;
        }
      }
    }
  }
  return results;
}

function extractRange(
  lines: string[],
  startLine: number,
  startCol: number,
  endLine: number,
  endCol: number,
): string {
  if (startLine === endLine) return lines[startLine].substring(startCol, endCol);
  const parts = [lines[startLine].substring(startCol)];
  for (let i = startLine + 1; i < endLine; i++) parts.push(lines[i]);
  parts.push(lines[endLine].substring(0, endCol));
  return parts.join("\n");
}

function lintContent() {
  if (!editor) return;
  const model = editor.getModel();
  if (!model) return;

  // 当前 tab 标记了 skipLint (例如 "查看索引" 展示型 tab), 清掉所有 marker 直接返回
  if (editorStore.activeTab?.skipLint) {
    monaco.editor.setModelMarkers(model, "mongo-lint", []);
    return;
  }

  const content = editor.getValue();
  const lines = content.split("\n");
  const markers: monaco.editor.IMarkerData[] = [];

  // 先检查非语句行（不以 db./use 开头且不是续行）
  const statements = parseStatements(content);

  // 检查空内容
  if (!content.trim()) {
    monaco.editor.setModelMarkers(model, "mongo-lint", []);
    return;
  }

  // 标记哪些行属于某个语句或注释
  const coveredLines = new Set<number>();
  for (const stmt of statements) {
    for (let l = stmt.startLine; l <= stmt.endLine; l++) coveredLines.add(l);
  }
  const commentLines = buildCommentSet(lines);
  // top-level JS 代码块 (function / const / let / var 等), 也算"已覆盖"不报错
  const jsBlockLines = buildJsCodeBlocks(lines, commentLines);
  // 非空行如果不属于任何语句/注释/JS 块, 才标记为"无法识别"
  for (let i = 0; i < lines.length; i++) {
    if (
      lines[i].trim() &&
      !coveredLines.has(i + 1) &&
      !commentLines.has(i) &&
      !jsBlockLines.has(i + 1)
    ) {
      markers.push({
        severity: monaco.MarkerSeverity.Error,
        message: "无法识别的语句，需要以 db. 开头",
        startLineNumber: i + 1,
        startColumn: 1,
        endLineNumber: i + 1,
        endColumn: lines[i].length + 1,
      });
    }
  }

  for (const stmt of statements) {
    const text = stmt.text.trim();
    if (!text) continue;

    // 1. 检查括号 / 花括号 / 方括号匹配
    let parenDepth = 0;
    let braceDepth = 0;
    let bracketDepth = 0;
    let inString = false;
    let strChar = "";
    let unclosedStringLine = -1;
    let unclosedStringCol = -1;

    for (let li = stmt.startLine - 1; li <= stmt.endLine - 1 && li < lines.length; li++) {
      const line = lines[li];
      for (let ci = 0; ci < line.length; ci++) {
        const ch = line[ci];
        if (inString) {
          if (ch === strChar && line[ci - 1] !== "\\") inString = false;
          continue;
        }
        if (ch === '"' || ch === "'") {
          inString = true;
          strChar = ch;
          unclosedStringLine = li;
          unclosedStringCol = ci;
          continue;
        }
        if (ch === "(") parenDepth++;
        else if (ch === ")") {
          parenDepth--;
          if (parenDepth < 0) {
            markers.push({
              severity: monaco.MarkerSeverity.Error,
              message: "多余的 )",
              startLineNumber: li + 1, startColumn: ci + 1,
              endLineNumber: li + 1, endColumn: ci + 2,
            });
            parenDepth = 0;
          }
        } else if (ch === "{") braceDepth++;
        else if (ch === "}") {
          braceDepth--;
          if (braceDepth < 0) {
            markers.push({
              severity: monaco.MarkerSeverity.Error,
              message: "多余的 }",
              startLineNumber: li + 1, startColumn: ci + 1,
              endLineNumber: li + 1, endColumn: ci + 2,
            });
            braceDepth = 0;
          }
        } else if (ch === "[") bracketDepth++;
        else if (ch === "]") {
          bracketDepth--;
          if (bracketDepth < 0) {
            markers.push({
              severity: monaco.MarkerSeverity.Error,
              message: "多余的 ]",
              startLineNumber: li + 1, startColumn: ci + 1,
              endLineNumber: li + 1, endColumn: ci + 2,
            });
            bracketDepth = 0;
          }
        }
      }
    }

    // 未关闭的字符串
    if (inString && unclosedStringLine >= 0) {
      markers.push({
        severity: monaco.MarkerSeverity.Error,
        message: `未关闭的字符串 ${strChar}`,
        startLineNumber: unclosedStringLine + 1,
        startColumn: unclosedStringCol + 1,
        endLineNumber: unclosedStringLine + 1,
        endColumn: lines[unclosedStringLine].length + 1,
      });
    }

    const lastLine = stmt.endLine;
    const lastLineText = lines[lastLine - 1] || "";
    if (parenDepth > 0) {
      markers.push({
        severity: monaco.MarkerSeverity.Error,
        message: `缺少 ${parenDepth} 个 )`,
        startLineNumber: lastLine, startColumn: lastLineText.length,
        endLineNumber: lastLine, endColumn: lastLineText.length + 1,
      });
    }
    if (braceDepth > 0) {
      markers.push({
        severity: monaco.MarkerSeverity.Error,
        message: `缺少 ${braceDepth} 个 }`,
        startLineNumber: lastLine, startColumn: lastLineText.length,
        endLineNumber: lastLine, endColumn: lastLineText.length + 1,
      });
    }
    if (bracketDepth > 0) {
      markers.push({
        severity: monaco.MarkerSeverity.Error,
        message: `缺少 ${bracketDepth} 个 ]`,
        startLineNumber: lastLine, startColumn: lastLineText.length,
        endLineNumber: lastLine, endColumn: lastLineText.length + 1,
      });
    }

    // 2. 括号匹配才继续检查参数内容
    if (parenDepth === 0 && braceDepth === 0 && bracketDepth === 0 && !inString) {
      const args = extractMethodArgs(stmt.text, stmt.startLine);
      for (const arg of args) {
        // 跳过纯数字参数 (limit(100), skip(0))
        if (/^\d+$/.test(arg.argText)) continue;
        // 跳过字符串参数 ("fieldName")
        if (/^["'][^"']*["']$/.test(arg.argText)) continue;

        // 尝试解析 {} 或 [] 参数
        // argText 可能是多个逗号分隔的参数 (如 updateOne(filter, update)),
        // 所以用 [argText] 当 JSON 数组解析, 兼容单参和多参.
        if (arg.argText.startsWith("{") || arg.argText.startsWith("[")) {
          try {
            JSON.parse(`[${relaxJsonForValidation(arg.argText)}]`);
          } catch (e) {
            const errMsg = String(e).replace(/^SyntaxError:\s*/, "");
            // 定位到参数在编辑器中的实际位置
            // arg.line = 参数起始行（编辑器行号）
            // arg.col  = 参数起始列（编辑器列号，'(' 后第一个字符）
            const posMatch = errMsg.match(/position\s+(\d+)/i);
            let errLine = arg.line;
            let errCol = arg.col;

            if (posMatch) {
              const pos = parseInt(posMatch[1]);
              // 在原始 argText 中定位 (不用 relaxed 避免偏移)
              let count = 0;
              const argLines = arg.argText.split("\n");
              for (let al = 0; al < argLines.length; al++) {
                if (count + argLines[al].length + 1 > pos) {
                  errLine = arg.line + al;
                  const offsetInArgLine = pos - count;
                  // 第一行需要加上参数在编辑器中的起始列偏移
                  errCol = al === 0
                    ? arg.col + offsetInArgLine
                    : offsetInArgLine + 1;
                  break;
                }
                count += argLines[al].length + 1;
              }
            }

            // 标记整个有问题的参数区域（方便用户看到）
            const argEndLine = arg.line + arg.argText.split("\n").length - 1;
            const argLastLineText = arg.argText.split("\n").pop() || "";
            const argEndCol = arg.argText.split("\n").length === 1
              ? arg.col + argLastLineText.length
              : argLastLineText.length + 1;

            markers.push({
              severity: monaco.MarkerSeverity.Error,
              message: `JSON 语法错误: ${errMsg}`,
              startLineNumber: errLine,
              startColumn: Math.max(1, errCol),
              endLineNumber: argEndLine,
              endColumn: argEndCol,
            });
          }
        }
      }
    }

    // 3. 检查 db.xxx 格式是否正确
    const dbDotMatch = text.match(/^db\s*\.\s*(\w+)/);
    if (dbDotMatch) {
      const afterColl = text.substring(dbDotMatch[0].length).trim();
      if (afterColl && !afterColl.startsWith(".") && !afterColl.startsWith("(")) {
        markers.push({
          severity: monaco.MarkerSeverity.Error,
          message: `"${dbDotMatch[1]}" 后面应该是 .method()`,
          startLineNumber: stmt.startLine,
          startColumn: dbDotMatch[0].length + 1,
          endLineNumber: stmt.startLine,
          endColumn: lines[stmt.startLine - 1].length + 1,
        });
      }
    }
  }

  monaco.editor.setModelMarkers(model, "mongo-lint", markers);
}

function scheduleLint() {
  if (lintTimer) clearTimeout(lintTimer);
  lintTimer = setTimeout(lintContent, 300);
}

/** 格式化: 有选区只格式化选区, 否则格式化全文 */
function formatEditor() {
  if (!editor) return;
  const model = editor.getModel();
  if (!model) return;
  const sel = editor.getSelection();
  if (sel && !sel.isEmpty()) {
    const text = model.getValueInRange(sel);
    editor.executeEdits("mongo-format", [{ range: sel, text: formatMongoShell(text) }]);
  } else {
    editor.executeEdits("mongo-format", [
      { range: model.getFullModelRange(), text: formatMongoShell(model.getValue()) },
    ]);
  }
}

function handleRun() {
  // Ctrl+Enter 时编辑器仍有焦点，可以直接读选区
  if (editor) {
    const sel = editor.getSelection();
    if (sel && !sel.isEmpty()) {
      const text = editor.getModel()?.getValueInRange(sel) ?? "";
      if (text.trim()) {
        emit("run", text);
        return;
      }
    }
  }
  // 否则使用缓存的 _runTarget（工具栏按钮点击时选区可能已被清除）
  emit("run", _runTarget || getStatementByCursor());
}

onMounted(() => {
  if (!editorRef.value) return;
  editor = monaco.editor.create(editorRef.value, {
    value: props.modelValue,
    language: props.language || MONGO_LANGUAGE_ID,
    theme: MONGO_THEME_LIGHT,
    minimap: { enabled: editorSettings.minimap },
    fontSize: editorSettings.fontSize,
    lineNumbers: "on",
    scrollBeyondLastLine: false,
    automaticLayout: true,
    tabSize: 2,
    wordWrap: editorSettings.wordWrap ? "on" : "off",
    suggestOnTriggerCharacters: true,
    // 显式开启全部三个上下文 (字符串里 / 注释里 / 普通代码里) 都允许 quick suggestions.
    // 之前 `quickSuggestions: true` 在某些 Monaco 版本里会被解释成只在 "other" 上下文激活,
    // 脚本 tab 里若光标恰好被认成在字符串/注释里, 补全 widget 就不会弹.
    quickSuggestions: { other: "on", comments: "off", strings: "on" },
    snippetSuggestions: "inline",
    // 关闭 word-based 提示, 避免和我们的 provider 抢 suggest widget 的排序
    wordBasedSuggestions: "off",
    // 关键: suggest widget / hover / param-hints 用 fixed 定位挂到 body, 不会被
    // 父容器 (.split-pane { overflow: hidden }) 裁掉. 脚本 tab 内容很长, 光标到底
    // 部时, 默认 absolute 定位的弹窗会被 split 边界吃掉 -> 用户看不到补全.
    fixedOverflowWidgets: true,
  });

  // 关闭浏览器的原生拼写检查: 整个容器 + inputarea <textarea> 都设 spellcheck=false,
  // 避免 Chrome 对 Mongo shell 代码 / 中文字符串画红色波浪线.
  editorRef.value.setAttribute("spellcheck", "false");
  const inputArea = editorRef.value.querySelector<HTMLTextAreaElement>(
    "textarea.inputarea",
  );
  if (inputArea) {
    inputArea.spellcheck = false;
    inputArea.setAttribute("autocorrect", "off");
    inputArea.setAttribute("autocapitalize", "off");
  }

  statementDecorations = editor.createDecorationsCollection([]);

  editor.onDidChangeModelContent((e) => {
    emit("update:modelValue", editor?.getValue() || "");
    updateStatementHighlight();
    updateRunTarget();
    scheduleLint();
    // 用户敲触发字符 -> 强制弹补全, 防止 Monaco 在某些状态下 suggestOnTriggerCharacters
    // 失活 (例如多 tab 切换 / HMR 后) 导致 `db.user.` 后没提示.
    for (const ch of e.changes) {
      if (ch.text === "." || ch.text === "$" || ch.text === "{") {
        // setTimeout 0 让 Monaco 先处理完模型变更再触发, 否则上下文 textBefore 还没包含新字符
        setTimeout(() => editor?.trigger("mongopilot", "editor.action.triggerSuggest", {}), 0);
        break;
      }
    }
  });

  // 初始化
  scheduleLint();
  _runTarget = getStatementByCursor();

  editor.onDidChangeCursorPosition(() => {
    updateStatementHighlight();
    updateRunTarget();
  });

  editor.onDidChangeCursorSelection(() => {
    updateRunTarget();
    syncSelection();
  });

  editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => handleRun());
  // Ctrl+Shift+Enter -> 运行所有语句
  editor.addCommand(
    monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.Enter,
    () => emit("runAll"),
  );
  // Ctrl+/ -> 注释/取消注释 (mongosh 语言已配 comments, 这里显式绑定确保生效)
  editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Slash, () => {
    editor?.getAction("editor.action.commentLine")?.run();
  });

  // 右键菜单: 格式化
  editor.addAction({
    id: "mongopilot.format",
    label: "格式化 (Beautify)",
    keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyMod.Alt | monaco.KeyCode.KeyL],
    contextMenuGroupId: "1_modification",
    contextMenuOrder: 1.5,
    run: () => formatEditor(),
  });

  // 右键菜单: AI 分析选中代码 (仅在有选区时出现)
  editor.addAction({
    id: "mongopilot.ai-analyze-selection",
    label: "✦ AI 分析选中代码",
    precondition: "editorHasSelection",
    contextMenuGroupId: "navigation",
    contextMenuOrder: 0,
    run: () => {
      syncSelection();
      emit("aiAnalyze");
    },
  });
});

// 工具栏 Format 按钮 -> 触发格式化
watch(
  () => props.formatTrigger,
  () => formatEditor(),
);

// 编辑器外观设置变化 -> 实时应用到所有编辑器实例
watch(
  editorSettings,
  (s) => {
    editor?.updateOptions({
      fontSize: s.fontSize,
      wordWrap: s.wordWrap ? "on" : "off",
      minimap: { enabled: s.minimap },
    });
  },
  { deep: true },
);

watch(() => props.modelValue, (newVal) => {
  if (editor && editor.getValue() !== newVal) editor.setValue(newVal);
});

onBeforeUnmount(() => {
  editor?.dispose();
  diffEditor?.dispose();
  diffModels?.original.dispose();
  diffModels?.modified.dispose();
});
</script>

<template>
  <div class="monaco-wrap">
    <div
      v-show="!hasPendingEdit"
      ref="editorRef"
      class="monaco-editor-container"
      spellcheck="false"
    />
    <div v-show="hasPendingEdit" class="diff-wrap">
      <div class="diff-bar">
        <span class="diff-bar-label">✦ AI 提议修改 —— 确认后才会应用到编辑器</span>
        <div class="diff-bar-actions">
          <button class="diff-btn reject" @click="onRejectEdit">放弃</button>
          <button class="diff-btn accept" @click="onAcceptEdit">应用修改</button>
        </div>
      </div>
      <div ref="diffRef" class="diff-container" />
    </div>
  </div>
</template>

<style>
.monaco-wrap { width: 100%; height: 100%; min-height: 200px; }
.monaco-editor-container { width: 100%; height: 100%; min-height: 200px; }
/* 当前语句高亮（浅色主题） */
.current-statement-highlight {
  background: rgba(56, 117, 215, 0.08) !important;
  border-left: 2px solid #3875d7 !important;
}
/* 补全弹窗里代码片段图标染成橙色, 跟方法 (紫色六边形) / 字段 / 关键字明显区分 */
.monaco-editor .suggest-widget .codicon-symbol-snippet,
.monaco-editor .suggest-widget .codicon-symbol-snippet::before {
  color: #e8a838 !important;
}
.monaco-editor .suggest-widget .monaco-icon-label.snippet > .monaco-icon-label-container::before {
  color: #e8a838 !important;
}
/* AI 提议编辑的 diff 视图 */
.diff-wrap {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
}
.diff-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 5px 10px;
  background: #f0f7ff;
  border-bottom: 1px solid #cfe3fb;
  flex-shrink: 0;
}
.diff-bar-label {
  font-size: 12px;
  color: #2b6cb0;
  font-weight: 500;
}
.diff-bar-actions {
  display: flex;
  gap: 6px;
}
.diff-btn {
  font-size: 12px;
  padding: 3px 12px;
  border-radius: 4px;
  border: 1px solid transparent;
  cursor: pointer;
  font: inherit;
}
.diff-btn.accept {
  background: #2080f0;
  color: #fff;
}
.diff-btn.accept:hover {
  background: #4098fc;
}
.diff-btn.reject {
  background: #fff;
  color: #666;
  border-color: #d0d0d0;
}
.diff-btn.reject:hover {
  background: #f5f5f5;
}
.diff-container {
  flex: 1;
  min-height: 0;
}
</style>
