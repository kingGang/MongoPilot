<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, watch } from "vue";
import * as monaco from "monaco-editor";
// 显式导入语言支持（防止 Vite/esbuild tree-shaking 丢弃 side-effect imports）
import "monaco-editor/esm/vs/basic-languages/javascript/javascript.contribution";
import "monaco-editor/esm/vs/language/typescript/monaco.contribution";
import { registerMongoCompletions } from "@/utils/mongo-completions";
import type { FieldCompletionInfo } from "@/utils/mongo-completions";
import { useEditorStore } from "@/stores/editor";
import { useDatabaseStore } from "@/stores/database";
import * as aiApi from "@/api/ai";

const props = defineProps<{
  modelValue: string;
  language?: string;
  /** 每次递增时触发执行当前语句 */
  runTrigger?: number;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: string];
  run: [statement: string];
}>();

const editorRef = ref<HTMLDivElement>();
let editor: monaco.editor.IStandaloneCodeEditor | null = null;
let statementDecorations: monaco.editor.IEditorDecorationsCollection | null = null;

const editorStore = useEditorStore();
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

// 注册 MongoDB 自动补全（全局只注册一次）
registerMongoCompletions({
  collectionNames: () => {
    const tab = editorStore.activeTab;
    if (!tab) return [];
    const colls = dbStore.getCollections(tab.connectionId, tab.database);
    return colls.map((c) => c.name);
  },
  getFieldNames,
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

function parseStatements(content: string): StatementRange[] {
  const lines = content.split("\n");
  const commentLines = buildCommentSet(lines);
  const statements: StatementRange[] = [];
  let current: { startLine: number; lines: string[] } | null = null;

  for (let i = 0; i < lines.length; i++) {
    // 跳过注释行
    if (commentLines.has(i)) {
      if (current) {
        statements.push({
          startLine: current.startLine,
          endLine: current.startLine + current.lines.length - 1,
          text: current.lines.join("\n"),
        });
        current = null;
      }
      continue;
    }

    const line = lines[i];
    const trimmed = line.trim();

    if (trimmed.startsWith("db.") || trimmed.startsWith("use ")) {
      if (current) {
        statements.push({
          startLine: current.startLine,
          endLine: current.startLine + current.lines.length - 1,
          text: current.lines.join("\n"),
        });
      }
      current = { startLine: i + 1, lines: [line] };
    } else if (current && trimmed !== "") {
      current.lines.push(line);
    } else {
      if (current) {
        statements.push({
          startLine: current.startLine,
          endLine: current.startLine + current.lines.length - 1,
          text: current.lines.join("\n"),
        });
        current = null;
      }
    }
  }

  if (current) {
    statements.push({
      startLine: current.startLine,
      endLine: current.startLine + current.lines.length - 1,
      text: current.lines.join("\n"),
    });
  }

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
function relaxJsonForValidation(text: string): string {
  // 1. 将 MongoDB Shell 类型构造器替换为普通字符串，避免 JSON.parse 报错
  //    ObjectId("..."), ISODate("..."), NumberLong("..."), NumberDecimal("..."),
  //    UUID("..."), BinData(...), Timestamp(...), new Date("..."), RegExp(...)
  let result = text.replace(
    /(?:new\s+)?(?:ObjectId|ISODate|UUID|NumberLong|NumberInt|NumberDecimal|BinData|Timestamp|Date|RegExp)\s*\([^)]*\)/g,
    (m) => JSON.stringify(m),
  );
  // 正则字面量 /pattern/flags
  result = result.replace(/\/(?:[^/\\]|\\.)+\/[gimsuy]*/g, (m) => JSON.stringify(m));

  // 2. 给未加引号的 key 加引号
  result = result.replace(
    /([{,]\s*)([a-zA-Z_$][a-zA-Z0-9_$]*)(\s*:)/g,
    '$1"$2"$3',
  );
  return result;
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
  // 非空行如果不属于任何语句且不是注释，标记为错误
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].trim() && !coveredLines.has(i + 1) && !commentLines.has(i)) {
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
        if (arg.argText.startsWith("{") || arg.argText.startsWith("[")) {
          try {
            JSON.parse(relaxJsonForValidation(arg.argText));
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
    language: props.language || "javascript",
    theme: "vs",
    minimap: { enabled: false },
    fontSize: 14,
    lineNumbers: "on",
    scrollBeyondLastLine: false,
    automaticLayout: true,
    tabSize: 2,
    wordWrap: "on",
    suggestOnTriggerCharacters: true,
    quickSuggestions: true,
    snippetSuggestions: "inline",
  });

  statementDecorations = editor.createDecorationsCollection([]);

  editor.onDidChangeModelContent(() => {
    emit("update:modelValue", editor?.getValue() || "");
    updateStatementHighlight();
    updateRunTarget();
    scheduleLint();
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
  });

  editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => handleRun());
});

watch(() => props.modelValue, (newVal) => {
  if (editor && editor.getValue() !== newVal) editor.setValue(newVal);
});

onBeforeUnmount(() => { editor?.dispose(); });
</script>

<template>
  <div ref="editorRef" class="monaco-editor-container" />
</template>

<style>
.monaco-editor-container { width: 100%; height: 100%; min-height: 200px; }
/* 当前语句高亮（浅色主题） */
.current-statement-highlight {
  background: rgba(56, 117, 215, 0.08) !important;
  border-left: 2px solid #3875d7 !important;
}
</style>
