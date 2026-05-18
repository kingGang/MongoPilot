/**
 * mongosh 语句美化 (Beautify)
 *
 * 纯括号层级重排, 不做语义分析:
 *   - {} / [] 里的内容按层级换行缩进; 空 {} / [] 保持内联
 *   - 链式调用 .sort()/.limit() 各起一行
 *   - 字符串 / 模板串 / 正则 / 注释 原样保留
 *   - ; 作为语句分隔, 之后空一行
 * 解析失败时返回原文 (绝不丢内容)。
 */
export function formatMongoShell(code: string, indentUnit = "  "): string {
  try {
    return doFormat(code, indentUnit);
  } catch {
    return code;
  }
}

function doFormat(code: string, indentUnit: string): string {
  const n = code.length;
  let i = 0;
  let out = "";
  let depth = 0; // {} [] 嵌套层级
  /** 每一对 () 是否是"含结构体的方法参数" —— 这种里面的 , 要换行 */
  const parenStructured: boolean[] = [];

  const indent = () => indentUnit.repeat(Math.max(0, depth));
  const nl = () => "\n" + indent();
  /** 去掉 out 尾部的空白 (换行/空格), 用于 } 前不留多余空白 */
  const trimTrailWs = () => {
    out = out.replace(/[ \t]*$/, "");
  };

  /** 从 j 往后跳过空白, 返回第一个非空白字符的索引 */
  const nextNonWs = (j: number) => {
    while (j < n && /\s/.test(code[j])) j++;
    return j;
  };
  /** 判断从 open 处起的 (...) 内是否含 { 或 [ (粗略, 字符串感知) */
  const parenHasStruct = (open: number): boolean => {
    let d = 1;
    let j = open + 1;
    let inStr = false;
    let sc = "";
    while (j < n && d > 0) {
      const c = code[j];
      if (inStr) {
        if (c === "\\") j += 2;
        else {
          if (c === sc) inStr = false;
          j++;
        }
        continue;
      }
      if (c === '"' || c === "'" || c === "`") {
        inStr = true;
        sc = c;
      } else if (c === "(") d++;
      else if (c === ")") d--;
      else if (d === 1 && (c === "{" || c === "[")) return true;
      j++;
    }
    return false;
  };

  while (i < n) {
    const c = code[i];

    // ---- 字符串 ----
    if (c === '"' || c === "'" || c === "`") {
      const q = c;
      let s = c;
      i++;
      while (i < n) {
        const ch = code[i];
        s += ch;
        i++;
        if (ch === "\\" && i < n) {
          s += code[i];
          i++;
        } else if (ch === q) {
          break;
        }
      }
      out += s;
      continue;
    }

    // ---- 行注释 ----
    if (c === "/" && code[i + 1] === "/") {
      let s = "";
      while (i < n && code[i] !== "\n") {
        s += code[i];
        i++;
      }
      // 注释独占一行还是行尾: 看 out 末尾
      if (/\S[ \t]*$/.test(out)) out += " " + s;
      else {
        trimTrailWs();
        out += (out.endsWith("\n") || out === "" ? indent() : nl()) + s;
      }
      continue;
    }

    // ---- 块注释 ----
    if (c === "/" && code[i + 1] === "*") {
      let s = "/*";
      i += 2;
      while (i < n && !(code[i] === "*" && code[i + 1] === "/")) {
        s += code[i];
        i++;
      }
      s += "*/";
      i += 2;
      out += s;
      continue;
    }

    // ---- 正则字面量 (粗略: 前一个非空字符不是标识符/数字/) ] 时才当正则) ----
    if (c === "/") {
      const prev = out.replace(/\s+$/, "").slice(-1);
      if (!/[\w$)\]]/.test(prev)) {
        let s = "/";
        i++;
        let inClass = false;
        while (i < n) {
          const ch = code[i];
          s += ch;
          i++;
          if (ch === "\\" && i < n) {
            s += code[i];
            i++;
          } else if (ch === "[") inClass = true;
          else if (ch === "]") inClass = false;
          else if (ch === "/" && !inClass) break;
        }
        while (i < n && /[a-z]/i.test(code[i])) {
          s += code[i];
          i++;
        }
        out += s;
        continue;
      }
    }

    // ---- 结构括号 {} [] ----
    if (c === "{" || c === "[") {
      const close = c === "{" ? "}" : "]";
      const j = nextNonWs(i + 1);
      if (code[j] === close) {
        // 空结构: 内联
        out += c + close;
        i = j + 1;
        continue;
      }
      out += c;
      depth++;
      out += nl();
      i++;
      continue;
    }
    if (c === "}" || c === "]") {
      depth--;
      trimTrailWs();
      if (!out.endsWith("\n")) out += "\n";
      out += indent() + c;
      i++;
      continue;
    }

    // ---- 方法调用 / 分组括号 ----
    if (c === "(") {
      parenStructured.push(parenHasStruct(i));
      out += c;
      i++;
      continue;
    }
    if (c === ")") {
      parenStructured.pop();
      trimTrailWs();
      out += c;
      i++;
      continue;
    }

    // ---- 逗号 ----
    if (c === ",") {
      trimTrailWs();
      out += ",";
      i++;
      // 在 {} [] 里 -> 换行; 在结构化的 () 参数里 -> 也换行; 否则 ", "
      if (depth > 0) {
        out += nl();
      } else if (parenStructured[parenStructured.length - 1]) {
        out += nl();
      } else {
        out += " ";
      }
      // 跳过原文里逗号后的空白
      i = nextNonWs(i);
      continue;
    }

    // ---- 冒号 (对象键值) ----
    if (c === ":" && depth > 0) {
      trimTrailWs();
      out += ": ";
      i++;
      i = nextNonWs(i);
      continue;
    }

    // ---- 分号: 语句结束 ----
    if (c === ";") {
      trimTrailWs();
      out += ";";
      i++;
      i = nextNonWs(i);
      if (i < n) out += "\n\n";
      continue;
    }

    // ---- 链式调用 .method() 换行 (仅顶层, depth===0) ----
    if (c === "." && depth === 0 && parenStructured.length === 0) {
      // 前面是 ) 或标识符, 且这是一次链式调用 (后面跟 method( )
      const prev = out.replace(/\s+$/, "").slice(-1);
      const rest = code.slice(i + 1);
      if (prev === ")" && /^\s*[a-zA-Z_$][\w$]*\s*\(/.test(rest)) {
        // 剥掉 out 末尾所有空白 (含换行): 上一轮 \n 处理器可能已经吐出 \n\n,
        // 这里要消掉, 避免链方法之间出现空行 (会让 lint 把 .method() 当孤儿)
        out = out.replace(/\s+$/, "");
        out += "\n" + indentUnit + ".";
        i++;
        i = nextNonWs(i);
        continue;
      }
    }

    // ---- 换行: 折叠 (我们自己控制换行); 但语句之间的空行保留为一个 ----
    if (c === "\n") {
      // 顶层连续换行 -> 保留一个空行作为语句分隔
      if (depth === 0 && parenStructured.length === 0) {
        const j = nextNonWs(i);
        // 下一非空白是链式 .method( -> 交给上面的 . 处理器, 这里不吐换行
        if (j < n && code[j] === "." && /^\.\s*[a-zA-Z_$][\w$]*\s*\(/.test(code.slice(j))) {
          i = j;
          continue;
        }
        // 原文这里有空行 (两个以上换行) 且不在结构里 -> 留一个空行
        const hadBlank = code.slice(i, j).split("\n").length > 2;
        trimTrailWs();
        if (out && !out.endsWith("\n")) out += hadBlank ? "\n\n" : "\n";
        i = j;
        continue;
      }
      i++;
      continue;
    }

    // ---- 普通空白: 折叠多余空格 ----
    if (c === " " || c === "\t" || c === "\r") {
      if (!/[\s(]$/.test(out)) out += " ";
      i++;
      continue;
    }

    out += c;
    i++;
  }

  return (
    out
      .replace(/[ \t]+\n/g, "\n")
      .replace(/\n{3,}/g, "\n\n")
      .trim() + "\n"
  );
}
