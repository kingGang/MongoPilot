/**
 * 查询预求值 (pre-evaluation)
 *
 * MongoPilot 的后端执行器是 Rust 写的 shell 语法解析器, 不跑任意 JS.
 * 当用户在编辑器里定义了 helper 函数 (例如 base64 加解密) 并在查询参数里调用时,
 * 后端无法识别. 这里在发送给后端**之前**, 用 webview 自带的 JS 引擎:
 *   1. 注入 shell 类型 stub (ObjectId/ISODate/...) + 浏览器缺失的 Node 全局 (Buffer)
 *   2. 执行用户的 helper 定义
 *   3. 用一个 `db` 代理捕获 `db.coll.method(<JS表达式>)` 调用, 参数被 JS 求值
 *   4. 把求值后的参数 JSON 化, 重建一条纯 JSON 参数的等价语句发给后端
 *
 * 安全性: 这等价于对用户**自己写的**脚本做 eval. MongoPilot 是本地客户端,
 * 用户运行的是自己的代码, 不涉及不可信输入, 与 mongosh 行为一致.
 */

/** shell 类型 / Node Buffer 的 stub —— 预求值和脚本模式共用 */
const TYPE_STUBS = `
const ObjectId = (x) => ({ $oid: x === undefined || x === null ? '0'.repeat(24) : String(x) });
const ISODate = (x) => ({ $date: x === undefined || x === null ? new Date().toISOString() : String(x) });
const NumberLong = (x) => ({ $numberLong: String(x) });
const NumberInt = (x) => Number(x);
const NumberDecimal = (x) => ({ $numberDecimal: String(x) });
const Double = (x) => Number(x);
// mongosh 的 UUID(): 无参随机生成, 带 .hex() / .toString(); JSON 化时输出 {$uuid:...}
const UUID = (x) => {
  let hex;
  if (x === undefined || x === null) {
    hex = '';
    for (let i = 0; i < 32; i++) hex += Math.floor(Math.random() * 16).toString(16);
  } else {
    hex = String(x).replace(/-/g, '').toLowerCase();
  }
  const dashed = hex.replace(/^(.{8})(.{4})(.{4})(.{4})(.{12})$/, '$1-$2-$3-$4-$5');
  return { $uuid: dashed, hex: () => hex, toString: () => dashed };
};
// 浏览器没有 Node 的 Buffer; 给一个最小实现, 覆盖 utf8 / base64 / hex 互转
const Buffer = {
  from(input, encoding) {
    let bytes;
    if (encoding === 'base64') {
      const bin = atob(String(input));
      bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
    } else if (encoding === 'hex') {
      const s = String(input);
      bytes = new Uint8Array(Math.floor(s.length / 2));
      for (let i = 0; i < bytes.length; i++) bytes[i] = parseInt(s.substr(i * 2, 2), 16);
    } else {
      bytes = new TextEncoder().encode(String(input));
    }
    return {
      toString(enc) {
        if (enc === 'base64') {
          let bin = '';
          for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
          return btoa(bin);
        }
        if (enc === 'hex') {
          let h = '';
          for (let i = 0; i < bytes.length; i++) h += bytes[i].toString(16).padStart(2, '0');
          return h;
        }
        return new TextDecoder().decode(bytes);
      },
    };
  },
};
`;

/** preEvaluateStatement 用的完整 prelude: 类型 stub + load/print/printjson 空操作 */
const PRELUDE = `
${TYPE_STUBS}
const load = () => true;
const print = () => undefined;
const printjson = () => undefined;
`;

/** 编辑器内容里是否存在 helper 定义 (function / const / let / var / class) */
export function hasHelperDefinitions(content: string): boolean {
  return (
    /(^|\n)[ \t]*(?:async[ \t]+)?function[ \t]/.test(content) ||
    /(^|\n)[ \t]*(?:const|let|var|class)[ \t]/.test(content)
  );
}

/** 提取内容里所有 load("...") / load('...') 调用的文件路径 (排除 foo.load() 这种方法调用) */
export function extractLoadPaths(content: string): string[] {
  const paths: string[] = [];
  const re = /(?<![.\w$])load\s*\(\s*(['"])([^'"]+?)\1\s*\)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(content)) !== null) {
    paths.push(m[2]);
  }
  return paths;
}

/** 是否需要走预求值: 有 helper 定义 或 有 load() 引用 */
export function needsPreEvaluation(content: string): boolean {
  return hasHelperDefinitions(content) || extractLoadPaths(content).length > 0;
}

/** 单行的括号净变化, 跳过字符串和行内 // 注释 */
function bracketDelta(line: string): number {
  let d = 0;
  let inStr = false;
  let strCh = "";
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (inStr) {
      if (c === "\\") {
        i++;
        continue;
      }
      if (c === strCh) inStr = false;
      continue;
    }
    if (c === '"' || c === "'") {
      inStr = true;
      strCh = c;
      continue;
    }
    if (c === "/" && line[i + 1] === "/") break;
    if (c === "(" || c === "{" || c === "[") d++;
    else if (c === ")" || c === "}" || c === "]") d--;
  }
  return d;
}

/** 去掉 fullContent 里所有 db.* / use 语句 (替换成空行保持行号), 只留 helper 定义 + 注释.
 *  能正确吞掉链式调用 (`.projection()` / `.sort()` 这种续行) 和语句内空行. */
function stripDbStatements(content: string): string {
  const lines = content.split("\n");
  const out: string[] = [];
  let depth = 0;
  let inStmt = false;

  const startStmt = (line: string) => {
    inStmt = true;
    depth = bracketDelta(line);
    out.push("");
  };

  for (const line of lines) {
    const trimmed = line.trim();
    if (!inStmt) {
      if (trimmed.startsWith("db.") || trimmed.startsWith("use ")) {
        startStmt(line);
      } else {
        out.push(line);
      }
      continue;
    }
    // inStmt 中
    if (trimmed === "") {
      out.push("");
      if (depth <= 0) inStmt = false;
      continue;
    }
    if (depth > 0 || trimmed.startsWith(".")) {
      // 续行: 括号未闭合, 或链式调用 .xxx()
      out.push("");
      depth += bracketDelta(line);
      continue;
    }
    // 不是续行 -> 上一条语句结束, 重新判定当前行
    inStmt = false;
    if (trimmed.startsWith("db.") || trimmed.startsWith("use ")) {
      startStmt(line);
    } else {
      out.push(line);
    }
  }
  return out.join("\n");
}

/**
 * 从一段文本里抽出所有顶层 db.* / use 语句 (含 `.projection()` 之类链式续行).
 * 与 stripDbStatements 是镜像逻辑: 那个把 db 语句抹掉, 这个把它们收集出来。
 * 用途: 用户选中了 "helper 函数 + 一条查询" 整段来执行时, 抽出真正要跑的查询语句。
 */
export function extractDbStatements(content: string): string[] {
  const lines = content.split("\n");
  const statements: string[] = [];
  let current: string[] = [];
  let depth = 0;
  let inStmt = false;

  const startStmt = (line: string) => {
    inStmt = true;
    depth = bracketDelta(line);
    current = [line];
  };
  const endStmt = () => {
    const s = current.join("\n").trim();
    if (s) statements.push(s);
    current = [];
    inStmt = false;
  };

  for (const line of lines) {
    const trimmed = line.trim();
    if (!inStmt) {
      if (trimmed.startsWith("db.") || trimmed.startsWith("use ")) startStmt(line);
      continue;
    }
    if (trimmed === "") {
      if (depth <= 0) endStmt();
      else current.push(line);
      continue;
    }
    if (depth > 0 || trimmed.startsWith(".")) {
      // 续行: 括号未闭合, 或链式调用 .xxx()
      current.push(line);
      depth += bracketDelta(line);
      continue;
    }
    // 不是续行 -> 上一条结束, 重新判定当前行
    endStmt();
    if (trimmed.startsWith("db.") || trimmed.startsWith("use ")) startStmt(line);
  }
  if (inStmt) endStmt();
  return statements;
}

interface Captured {
  collRender: string;
  method: string;
  args: unknown[];
  chain: { method: string; args: unknown[] }[];
}

/**
 * 抽出脚本里**纯 helper 部分**: function 声明 + 不引用 db 的 var/let/const 赋值.
 *
 * 用途: 用户在 helper-rich 脚本里**选了一行非 db.* 代码**跑 (例如 `print(...)`),
 * 期望"只跑这一行, 但让 helper 函数和上面的字面量 var 可见"。直接用整段 tab.content
 * 跑会把整篇脚本的 print / 控制流全跑掉, 这里只留下声明类的行。
 *
 * 保留:
 *   - `function fn(...) { ... }` (整段)
 *   - `var/let/const X = <不含 db. 的表达式>;`
 * 转换:
 *   - `var X = db.xxx(...)` -> `var X;` (只声明不赋值, 给 selection 用 X 时是 undefined)
 * 丢弃:
 *   - 控制流 (if/for/while/try/...)
 *   - `db.xxx.method(...)` 独立调用
 *   - `print(...)` / 其它表达式语句
 *   - 注释 (整体保留行号意义不大, 直接清掉)
 */
export function extractPureHelpers(content: string): string {
  const lines = content.split("\n");
  const out: string[] = [];
  let inFn = false;
  let fnBuf: string[] = [];
  let fnDepth = 0;

  for (const line of lines) {
    if (inFn) {
      fnBuf.push(line);
      fnDepth += bracketDelta(line);
      if (fnDepth <= 0) {
        out.push(fnBuf.join("\n"));
        fnBuf = [];
        inFn = false;
      }
      continue;
    }
    const trimmed = line.trim();
    // function 声明 (可能跨行) - 整段保留
    if (/^(?:async\s+)?function\s+[\w$]/.test(trimmed)) {
      inFn = true;
      fnBuf = [line];
      fnDepth = bracketDelta(line);
      if (fnDepth <= 0) {
        out.push(fnBuf.join("\n"));
        fnBuf = [];
        inFn = false;
      }
      continue;
    }
    // var/let/const X = ... - 看 RHS 有没有 db.
    const declMatch = trimmed.match(/^(?:var|let|const)\s+([\w$]+)\b/);
    if (declMatch) {
      if (/\bdb\s*[.[]/.test(line)) {
        // RHS 引用了 db, 只保留声明, 让 selection 引用时是 undefined
        out.push(`var ${declMatch[1]};`);
      } else {
        // 字面量 / 调 helper 函数等纯计算, 整行保留
        out.push(line);
      }
      continue;
    }
    // 其它一律丢
  }
  if (inFn && fnBuf.length > 0) out.push(fnBuf.join("\n"));
  return out.join("\n");
}

/**
 * 把 helperCode 改写成 "声明外提 + 执行体放进 IIFE 吃掉 return" 的形式.
 *
 *   var X = ...;            ->  let X; 在外层 + `X = ...;` 在 IIFE
 *   function fn(...) { .. } ->  整段外提到外层 (函数声明本身是安全的, 不会副作用)
 *   if (...) { return; }    ->  保留, return 只退出 IIFE, 不会把 target statement 截断
 *
 * 修复场景: 用户写的脚本通常是命令式的, 里面会有 `var p = db.player.findOne(...)`
 * 配合 `if (!p) { print(...); return; }` 这种早退. preEvaluateStatement 原本把整段
 * 当作 prelude 跑, 顶层 `return` 会把外层包装函数也一并退掉, target 语句根本没机会
 * 跑 -> holder.cap 为 null -> 返回原始未求值语句 -> 后端拿到裸标识符报 "expected value".
 */
function rewriteHelperForSafeEval(content: string): { hoisted: string; body: string } {
  const lines = content.split("\n");
  const hoistedNames = new Set<string>();
  const hoistedFunctions: string[] = [];
  const bodyLines: string[] = [];

  let inFn = false;
  let fnBuf: string[] = [];
  let fnDepth = 0;

  for (const line of lines) {
    if (inFn) {
      fnBuf.push(line);
      fnDepth += bracketDelta(line);
      bodyLines.push(""); // 占位保持行号
      if (fnDepth <= 0) {
        hoistedFunctions.push(fnBuf.join("\n"));
        fnBuf = [];
        inFn = false;
      }
      continue;
    }

    const trimmed = line.trim();

    // function 声明 (可能跨行)
    if (/^(?:async\s+)?function\s+[\w$]/.test(trimmed)) {
      inFn = true;
      fnBuf = [line];
      fnDepth = bracketDelta(line);
      bodyLines.push("");
      if (fnDepth <= 0) {
        hoistedFunctions.push(fnBuf.join("\n"));
        fnBuf = [];
        inFn = false;
      }
      continue;
    }

    // var/let/const X = ... -> 把 X 提到外层 let, 行内只留赋值
    const declMatch = line.match(/^(\s*)(?:var|let|const)\s+([\w$]+)\b/);
    if (declMatch) {
      hoistedNames.add(declMatch[2]);
      bodyLines.push(line.replace(/^(\s*)(?:var|let|const)\s+([\w$]+)/, "$1$2"));
      continue;
    }

    bodyLines.push(line);
  }
  // 没闭合的函数: 还是放进 hoisted, 让 new Function 编译时报错被外层 catch 吞
  if (inFn && fnBuf.length > 0) hoistedFunctions.push(fnBuf.join("\n"));

  const hoistedDecl = hoistedNames.size > 0 ? `let ${[...hoistedNames].join(", ")};` : "";
  return {
    hoisted: `${hoistedDecl}\n${hoistedFunctions.join("\n")}`,
    body: bodyLines.join("\n"),
  };
}

/**
 * 预求值: 返回参数已被求值成 JSON 字面量的等价语句.
 * 求值失败 / 语句不是 db.* 形式时, 原样返回 statement (让后端按原逻辑处理或报错).
 *
 * @param loadedHelpers  由 load("...") 引入的外部文件内容 (已读取好的拼接字符串).
 *                       会先于编辑器内的 helper 注入作用域, 模拟 mongosh load() 语义.
 */
export function preEvaluateStatement(
  fullContent: string,
  statement: string,
  loadedHelpers = "",
): string {
  // load() 引入的文件也可能含 db.* 语句, 一并 strip 掉, 只留定义
  const loadedCode = loadedHelpers ? stripDbStatements(loadedHelpers) : "";
  const helperCode = `${loadedCode}\n${stripDbStatements(fullContent)}`;
  // 把 helperCode 改写: 声明外提, 执行体进 IIFE 吃掉早 return / 抛错
  const { hoisted, body: safeHelperBody } = rewriteHelperForSafeEval(helperCode);
  const holder: { cap: Captured | null } = { cap: null };

  const body = `
${PRELUDE}
${hoisted}
const __mkCursor__ = (cap) => new Proxy({}, {
  get(_, m) {
    return (...a) => { cap.chain.push({ method: String(m), args: a }); return __mkCursor__(cap); };
  },
});
const __mkColl__ = (render) => new Proxy({}, {
  get(_, m) {
    return (...a) => {
      const cap = { collRender: render, method: String(m), args: a, chain: [] };
      __onCapture__(cap);
      return __mkCursor__(cap);
    };
  },
});
const db = new Proxy({}, {
  get(_, prop) {
    if (prop === 'getCollection') {
      return (name) => __mkColl__('db.getCollection(' + JSON.stringify(String(name)) + ')');
    }
    if (prop === 'getSiblingDB') {
      return () => db;
    }
    return __mkColl__('db.' + String(prop));
  },
});
/* helperCode 放进 IIFE: 顶层 return 只退出 IIFE; 抛错也被吞掉.
   vars 已经在上面 hoisted 里用 let 提到外层了, IIFE 里只是赋值, 退出后 target 还能读到. */
(() => {
  try {
    ${safeHelperBody}
  } catch (e) { /* helperCode 出错不影响 target statement 求值 */ }
})();
${statement}
`;

  try {
    // eslint-disable-next-line no-new-func
    const fn = new Function("__onCapture__", body);
    fn((c: Captured) => {
      holder.cap = c;
    });
  } catch (e) {
    // helperCode 解析/执行失败时 fallback 到原 statement, 同时把错误打到 DevTools.
    console.warn("[preEvaluateStatement] eval failed, falling back to raw statement:", e);
    return statement;
  }

  const c = holder.cap;
  if (!c) {
    return statement;
  }

  const renderArgs = (args: unknown[]) =>
    args.map((a) => (a === undefined ? "undefined" : JSON.stringify(a))).join(", ");

  let rebuilt = `${c.collRender}.${c.method}(${renderArgs(c.args)})`;
  for (const ch of c.chain) {
    rebuilt += `.${ch.method}(${renderArgs(ch.args)})`;
  }
  return rebuilt;
}

// ============================================================================
// 脚本模式: 整段命令式脚本 (load + var + for + 函数调用 + 读后写)
// 在 webview 里完整跑一遍:
//   - db **读**操作 (顶层的) 真去后端拿数据 —— read-then-write 脚本能拿到真实 _id 等;
//   - db **写**操作收集进 ops (不立即执行), 跑完后由调用方批量发后端;
//   - print()/printjson() 输出被捕获。
// 关键难点: db 调用可能在普通 function 里 (如生成脚本), 那里不能加 await ——
// awaitifyDbCalls 只给**不在 function 体内**的 db 调用加 await; function 里的写操作
// 靠 async 代理方法被调用时同步 push 进 ops, 不需要 await。
// ============================================================================

/** 脚本模式收集到的一个 db 写操作 */
export interface ScriptOp {
  /** 集合渲染前缀, 如 `db.dolls` 或 `db.getCollection("a.b")` */
  collRender: string;
  method: string;
  args: unknown[];
}

/** 脚本里 db 读操作的执行器: 把重建好的 `db.coll.method(JSON...)` 语句发后端跑 */
export type ScriptReadRunner = (
  statement: string,
) => Promise<{ documents: Record<string, unknown>[]; count: number }>;

/** collectScriptOps 的结果 */
export interface ScriptRunResult {
  /** 收集到的写操作 (待调用方执行) */
  ops: ScriptOp[];
  /** print() / printjson() 的输出 */
  output: string[];
  /** 脚本执行抛错时的信息; null 表示成功跑完 */
  error: string | null;
}

/** 会改库的方法 —— 脚本模式里这些被收集, 其它读方法返回安全默认值 */
const WRITE_METHODS = new Set([
  "insertOne",
  "insertMany",
  "updateOne",
  "updateMany",
  "deleteOne",
  "deleteMany",
  "replaceOne",
  "save",
  "remove",
  "bulkWrite",
  "findOneAndUpdate",
  "findOneAndReplace",
  "findOneAndDelete",
]);

/** 去掉 mongosh 的 `use xxx` 行 (不是合法 JS), 换成空行保持行号 */
function stripUseStatements(content: string): string {
  return content
    .split("\n")
    .map((line) => (/^\s*use\s+\S/.test(line) ? "" : line))
    .join("\n");
}

/**
 * 字符串/注释感知地给**不在 function 体内**的顶层 db 引用 (db. / db[) 加上 await,
 * 让 mongosh 那种同步写法的 db 读操作能 await 真正的后端查询。
 * function 体内的 db 调用不动 —— 那里多数是写操作, async 代理方法被调用时会同步收集,
 * 不需要 await (在普通 function 里加 await 反而是语法错误)。
 */
function awaitifyDbCalls(code: string): string {
  let out = "";
  let i = 0;
  const n = code.length;
  let inStr = false;
  let strCh = "";
  let inLine = false;
  let inBlock = false;
  let braceDepth = 0;
  const funcDepths: number[] = []; // 进入 function 体时压入当时的 braceDepth
  let pendingFunc = false; // 见到 function 关键字, 等它的 {
  const isIdent = (ch: string) => /[\w$]/.test(ch || "");

  while (i < n) {
    const c = code[i];
    const next = code[i + 1];

    if (inLine) {
      out += c;
      if (c === "\n") inLine = false;
      i++;
      continue;
    }
    if (inBlock) {
      out += c;
      if (c === "*" && next === "/") {
        out += next;
        i += 2;
        inBlock = false;
        continue;
      }
      i++;
      continue;
    }
    if (inStr) {
      out += c;
      if (c === "\\" && i + 1 < n) {
        out += next;
        i += 2;
        continue;
      }
      if (c === strCh) inStr = false;
      i++;
      continue;
    }
    if (c === "/" && next === "/") {
      inLine = true;
      out += c;
      i++;
      continue;
    }
    if (c === "/" && next === "*") {
      inBlock = true;
      out += c;
      i++;
      continue;
    }
    if (c === '"' || c === "'" || c === "`") {
      inStr = true;
      strCh = c;
      out += c;
      i++;
      continue;
    }
    // function 关键字 -> 等它的 { 来标记函数体
    if (
      c === "f" &&
      code.startsWith("function", i) &&
      !isIdent(code[i - 1]) &&
      !isIdent(code[i + 8])
    ) {
      pendingFunc = true;
      out += "function";
      i += 8;
      continue;
    }
    if (c === "{") {
      braceDepth++;
      if (pendingFunc) {
        funcDepths.push(braceDepth);
        pendingFunc = false;
      }
      out += c;
      i++;
      continue;
    }
    if (c === "}") {
      if (funcDepths.length && funcDepths[funcDepths.length - 1] === braceDepth) {
        funcDepths.pop();
      }
      braceDepth--;
      out += c;
      i++;
      continue;
    }
    // 顶层 (不在任何 function 体内) 的 db. / db[ -> 加 await
    if (
      c === "d" &&
      next === "b" &&
      (code[i + 2] === "." || code[i + 2] === "[") &&
      !isIdent(code[i - 1]) &&
      code[i - 1] !== "." &&
      funcDepths.length === 0
    ) {
      out += "await db";
      i += 2;
      continue;
    }
    out += c;
    i++;
  }
  return out;
}

/**
 * 在 webview JS 引擎里**完整执行**一段命令式脚本。
 * - db 读操作 (findOne/find().toArray()/countDocuments/...) 真的发后端查询拿真实数据;
 * - db 写操作收集进 ops (不立即执行), 由调用方跑完后批量发后端;
 * - print()/printjson() 输出被捕获进 output。
 * 任意环节抛错 -> error 带信息, ops/output 给出错前已收集的部分。
 *
 * @param loadedHelpers  由 load("...") 引入的外部文件内容 (已拼接好)。
 * @param runRead        把重建好的 db 读语句发后端执行的回调。
 */
export async function collectScriptOps(
  fullContent: string,
  loadedHelpers: string,
  runRead: ScriptReadRunner,
): Promise<ScriptRunResult> {
  const merged = `${stripUseStatements(loadedHelpers)}\n${stripUseStatements(fullContent)}`;
  const code = awaitifyDbCalls(merged);

  const body = `
${TYPE_STUBS}
const load = () => true;
const __out__ = [];
const __ops__ = [];
const __fmt__ = (x) => (typeof x === 'object' && x !== null ? JSON.stringify(x) : String(x));
const print = (...a) => { __out__.push(a.map(__fmt__).join(' ')); };
const printjson = (x) => { __out__.push(JSON.stringify(x, null, 2)); };
const __renderArgs__ = (args) =>
  args.map((a) => (a === undefined ? 'undefined' : JSON.stringify(a))).join(', ');
const __ack__ = (method) => {
  if (method === 'insertOne') return { acknowledged: true, insertedId: { $oid: '0'.repeat(24) } };
  if (method === 'insertMany') return { acknowledged: true, insertedIds: {} };
  if (method === 'bulkWrite') return { acknowledged: true, insertedCount: 0, matchedCount: 0, modifiedCount: 0, deletedCount: 0, upsertedCount: 0 };
  if (method === 'findOneAndUpdate' || method === 'findOneAndReplace' || method === 'findOneAndDelete') return null;
  return { acknowledged: true, matchedCount: 0, modifiedCount: 0, deletedCount: 0, upsertedCount: 0 };
};
// __hydrate__: 把后端返回的 BSON Extended JSON ({$oid}/{$date}/{$numberLong}) 递归补上
// mongosh 风格的访问方法 (_id.str / date.getTime() 等), 让用户脚本能像 mongosh 一样写.
// 原 $xxx 字段保留, 不影响序列化回后端.
const __hydrate__ = (v) => {
  if (v === null || typeof v !== 'object') return v;
  if (Array.isArray(v)) { for (let i = 0; i < v.length; i++) v[i] = __hydrate__(v[i]); return v; }
  const keys = Object.keys(v);
  if (keys.length === 1 && typeof v.$oid === 'string') {
    const s = v.$oid;
    Object.defineProperty(v, 'str', { value: s, enumerable: false });
    v.toString = () => s;
    v.valueOf = () => s;
    return v;
  }
  if (keys.length === 1 && '$date' in v) {
    const raw = v.$date;
    let ms = NaN;
    if (typeof raw === 'string') ms = new Date(raw).getTime();
    else if (typeof raw === 'number') ms = raw;
    else if (raw && typeof raw.$numberLong === 'string') ms = parseInt(raw.$numberLong);
    if (!isNaN(ms)) {
      const d = new Date(ms);
      Object.defineProperty(v, 'getTime', { value: () => ms, enumerable: false });
      Object.defineProperty(v, 'toISOString', { value: () => d.toISOString(), enumerable: false });
      v.toString = () => d.toISOString();
      v.valueOf = () => ms;
    }
    return v;
  }
  if (keys.length === 1 && typeof v.$numberLong === 'string') {
    const n = v.$numberLong;
    v.toString = () => n;
    v.valueOf = () => Number(n);
    return v;
  }
  for (const k of keys) v[k] = __hydrate__(v[k]);
  return v;
};
// 游标: 链式方法累积, 终结方法 (toArray/forEach/...) 才真去后端查
const __mkCursor__ = (render, baseMethod, baseArgs) => {
  const chain = [];
  const stmt = () => {
    let s = render + '.' + baseMethod + '(' + __renderArgs__(baseArgs) + ')';
    for (const ch of chain) s += '.' + ch.method + '(' + __renderArgs__(ch.args) + ')';
    return s;
  };
  const cur = new Proxy(function () {}, {
    get(_, m) {
      const mm = String(m);
      if (mm === 'toArray') return async () => (await __runRead__(stmt())).documents.map(__hydrate__);
      if (mm === 'forEach') return async (fn) => { for (const d of (await __runRead__(stmt())).documents) fn(__hydrate__(d)); };
      if (mm === 'map') return async (fn) => (await __runRead__(stmt())).documents.map(__hydrate__).map(fn);
      if (mm === 'count' || mm === 'size' || mm === 'itcount' || mm === 'length')
        return async () => (await __runRead__(stmt())).count;
      if (mm === 'hasNext') return async () => (await __runRead__(stmt())).documents.length > 0;
      if (mm === 'next' || mm === 'tryNext')
        return async () => { const d = (await __runRead__(stmt())).documents; return d.length ? __hydrate__(d[0]) : null; };
      if (mm === 'pretty' || mm === 'explain' || mm === 'close') return () => cur;
      if (mm === Symbol.iterator) return function* () {};
      // 其它当链式 (sort/limit/skip/projection/...)
      return (...a) => { chain.push({ method: mm, args: a }); return cur; };
    },
  });
  return cur;
};
const __mkColl__ = (render) => new Proxy({}, {
  get(_, m) {
    const method = String(m);
    // find/aggregate 同步返回游标 (这样 .toArray() 能链上去再 await)
    if (method === 'find' || method === 'aggregate') {
      return (...args) => __mkCursor__(render, method, args);
    }
    return async (...args) => {
      if (__WRITE__.has(method)) {
        __ops__.push({ collRender: render, method, args });
        return __ack__(method);
      }
      if (method === 'findOne') {
        const r = await __runRead__(render + '.findOne(' + __renderArgs__(args) + ')');
        return r.documents.length ? __hydrate__(r.documents[0]) : null;
      }
      if (method === 'countDocuments' || method === 'count' || method === 'estimatedDocumentCount') {
        const r = await __runRead__(render + '.countDocuments(' + __renderArgs__(args) + ')');
        return r.count;
      }
      if (method === 'distinct') {
        const r = await __runRead__(render + '.distinct(' + __renderArgs__(args) + ')');
        return r.documents.map(__hydrate__);
      }
      // 未知方法当读处理
      const r = await __runRead__(render + '.find(' + __renderArgs__(args) + ')');
      return r.documents.map(__hydrate__);
    };
  },
});
const db = new Proxy({}, {
  get(_, prop) {
    if (prop === 'getCollection') {
      return (name) => __mkColl__('db.getCollection(' + JSON.stringify(String(name)) + ')');
    }
    if (prop === 'getSiblingDB') return () => db;
    if (prop === 'getName') return () => '';
    return __mkColl__('db.' + String(prop));
  },
});
let __err__ = null;
try {
  // 包一层 async IIFE: 用户脚本里的顶层 return; 只从这里返回, 不会跳过下面的收尾
  await (async () => {
${code}
  })();
} catch (e) {
  __err__ = e && e.message ? String(e.message) : String(e);
}
return { ops: __ops__, output: __out__, error: __err__ };
`;

  try {
    const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor as new (
      ...args: string[]
    ) => (...a: unknown[]) => Promise<unknown>;
    // eslint-disable-next-line new-cap
    const fn = new AsyncFunction("__WRITE__", "__runRead__", body);
    const result = (await fn(WRITE_METHODS, runRead)) as {
      ops?: unknown;
      output?: unknown;
      error?: unknown;
    };
    const ops: ScriptOp[] = Array.isArray(result?.ops)
      ? result.ops.map((op) => {
          const o = op as ScriptOp;
          return {
            collRender: String(o.collRender),
            method: String(o.method),
            args: Array.isArray(o.args) ? o.args : [],
          };
        })
      : [];
    const output: string[] = Array.isArray(result?.output)
      ? result.output.map((s) => String(s))
      : [];
    const error = result?.error != null ? String(result.error) : null;
    return { ops, output, error };
  } catch (e) {
    // 编译/运行失败时, 单独把 user code (含 loaded helpers) 拿出来再编译一次:
    // 这样报错行号就是 user code 自身的真实行号, 而不是被嵌进 body 后被 TYPE_STUBS / db
    // proxy 定义堆出来的相对位置, 用户能直接定位到自己脚本里的语法问题 (例如字符串内嵌
    // 双引号未转义).
    let detail = String(e);
    try {
      const AsyncFn = Object.getPrototypeOf(async function () {}).constructor as new (
        ...args: string[]
      ) => unknown;
      // 单独编译 user code (含 awaitifyDbCalls 处理过的版本); 不带任何 wrapping
      new AsyncFn(code);
      // user code 单独编译通过 -> 是 MongoPilot 包装层的 bug
      detail = `${String(e)} (MongoPilot 内部错误, 用户脚本本身可编译; 请把脚本贴给开发者)`;
    } catch (innerE) {
      // user code 单独编译就报错 -> 是用户脚本本身的语法问题
      detail = `用户脚本语法错: ${String(innerE)} (检查字符串内嵌引号是否成对、转义是否正确; 长字符串建议用 \` 模板字符串包)`;
    }
    return { ops: [], output: [], error: detail };
  }
}

/** 把收集到的脚本操作渲染成后端能执行的 `db.coll.method(JSON...)` 语句 */
export function scriptOpToStatement(op: ScriptOp): string {
  const renderArgs = op.args
    .map((a) => (a === undefined ? "undefined" : JSON.stringify(a)))
    .join(", ");
  return `${op.collRender}.${op.method}(${renderArgs})`;
}
