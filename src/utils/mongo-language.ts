import * as monaco from "monaco-editor";

export const MONGO_LANGUAGE_ID = "mongosh";
export const MONGO_THEME_LIGHT = "mongosh-light";

/** 在 vs (light) 基础上为我们自定义的 token 着色 */
export function registerMongoTheme(): void {
  monaco.editor.defineTheme(MONGO_THEME_LIGHT, {
    base: "vs",
    inherit: true,
    rules: [
      // MongoDB 操作符: $set / $push / $lookup ...
      { token: "keyword.operator.mongo", foreground: "af00db", fontStyle: "bold" },
      // BSON 类型构造器: ObjectId / ISODate / Double ...
      { token: "type.mongo", foreground: "267f99" },
      // Collection / Cursor / Database 方法: find / updateOne / aggregate ...
      { token: "tag", foreground: "795e26" },
      // db / use 顶级命名空间
      { token: "keyword.control", foreground: "0000ff", fontStyle: "bold" },
      // 常量: true / false / null / undefined
      { token: "constant.language", foreground: "0000ff" },
    ],
    colors: {},
  });
}

/** 注册 "mongosh" 语言: Monarch 语法高亮 + 括号/缩进配置. 只需调用一次. */
export function registerMongoLanguage(): void {
  const alreadyRegistered = monaco.languages.getLanguages().some((l) => l.id === MONGO_LANGUAGE_ID);
  if (alreadyRegistered) return;

  monaco.languages.register({ id: MONGO_LANGUAGE_ID });

  monaco.languages.setLanguageConfiguration(MONGO_LANGUAGE_ID, {
    comments: { lineComment: "//", blockComment: ["/*", "*/"] },
    brackets: [
      ["{", "}"],
      ["[", "]"],
      ["(", ")"],
    ],
    autoClosingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: '"', close: '"' },
      { open: "'", close: "'" },
      { open: "`", close: "`" },
    ],
    surroundingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: '"', close: '"' },
      { open: "'", close: "'" },
      { open: "`", close: "`" },
    ],
    folding: {
      markers: {
        start: /^\s*\/\/\s*#?region\b/,
        end: /^\s*\/\/\s*#?endregion\b/,
      },
    },
  });

  monaco.languages.setMonarchTokensProvider(MONGO_LANGUAGE_ID, {
    defaultToken: "",
    tokenPostfix: ".mongo",

    keywords: [
      "var",
      "let",
      "const",
      "if",
      "else",
      "return",
      "function",
      "for",
      "while",
      "do",
      "switch",
      "case",
      "break",
      "continue",
      "new",
      "typeof",
      "instanceof",
      "in",
      "of",
      "try",
      "catch",
      "finally",
      "throw",
      "this",
      "delete",
    ],

    literals: ["true", "false", "null", "undefined"],

    /** BSON 类型构造器 —— 用 type 令牌色 */
    mongoTypes: [
      "ObjectId",
      "ISODate",
      "NumberLong",
      "NumberDecimal",
      "NumberInt",
      "Double",
      "UUID",
      "BinData",
      "Timestamp",
      "Date",
      "RegExp",
    ],

    /** Collection / Cursor / Database 方法 —— 用 tag 令牌色 */
    mongoMethods: [
      // query
      "find",
      "findOne",
      "findOneAndUpdate",
      "findOneAndReplace",
      "findOneAndDelete",
      "aggregate",
      "countDocuments",
      "estimatedDocumentCount",
      "distinct",
      "explain",
      // mutation
      "insertOne",
      "insertMany",
      "updateOne",
      "updateMany",
      "replaceOne",
      "deleteOne",
      "deleteMany",
      "bulkWrite",
      // cursor chain
      "projection",
      "sort",
      "limit",
      "skip",
      "hint",
      "toArray",
      // index / ddl
      "createIndex",
      "createIndexes",
      "dropIndex",
      "dropIndexes",
      "listIndexes",
      "getIndexes",
      // collection / db
      "drop",
      "stats",
      "renameCollection",
      "runCommand",
      "getCollection",
      "getSiblingDB",
      // users / roles
      "getUser",
      "getUsers",
      "createUser",
      "dropUser",
      "getRole",
      "getRoles",
      "createRole",
      "dropRole",
      "updateUser",
      "grantRolesToUser",
      "revokeRolesFromUser",
    ],

    /** 顶级 db / use */
    mongoGlobals: ["db", "use"],

    symbols: /[=><!~?:&|+\-*\/\^%]+/,

    tokenizer: {
      root: [
        // MongoDB 操作符: $set, $push, $inc, $lookup, $match 等
        [/\$[a-zA-Z_][a-zA-Z0-9_]*/, "keyword.operator.mongo"],

        // 标识符分类
        [
          /[a-zA-Z_][a-zA-Z0-9_]*/,
          {
            cases: {
              "@mongoTypes": "type.mongo",
              "@mongoMethods": "tag",
              "@mongoGlobals": "keyword.control",
              "@keywords": "keyword",
              "@literals": "constant.language",
              "@default": "identifier",
            },
          },
        ],

        // 空白 / 注释
        { include: "@whitespace" },

        // 数字
        [/0[xX][0-9a-fA-F]+/, "number.hex"],
        [/\d*\.\d+([eE][\-+]?\d+)?/, "number.float"],
        [/\d+/, "number"],

        // 字符串
        [/"([^"\\]|\\.)*$/, "string.invalid"],
        [/'([^'\\]|\\.)*$/, "string.invalid"],
        [/`/, { token: "string.quote", bracket: "@open", next: "@tplstring" }],
        [/"/, { token: "string.quote", bracket: "@open", next: "@dqstring" }],
        [/'/, { token: "string.quote", bracket: "@open", next: "@sqstring" }],

        // 正则字面量 (简化: /.../flags, 不跨行)
        [/\/(?:[^/\\\n]|\\.)+\/[gimsuy]*/, "regexp"],

        // 分隔符 / 操作符
        [/[{}()\[\]]/, "@brackets"],
        [/[;,.]/, "delimiter"],
        [/@symbols/, "operator"],
      ],

      whitespace: [
        [/[ \t\r\n]+/, ""],
        [/\/\*/, "comment", "@blockComment"],
        [/\/\/.*$/, "comment"],
      ],

      blockComment: [
        [/[^*/]+/, "comment"],
        [/\*\//, "comment", "@pop"],
        [/[\/*]/, "comment"],
      ],

      dqstring: [
        [/[^\\"]+/, "string"],
        [/\\./, "string.escape"],
        [/"/, { token: "string.quote", bracket: "@close", next: "@pop" }],
      ],
      sqstring: [
        [/[^\\']+/, "string"],
        [/\\./, "string.escape"],
        [/'/, { token: "string.quote", bracket: "@close", next: "@pop" }],
      ],
      tplstring: [
        [/[^\\`$]+/, "string"],
        [/\\./, "string.escape"],
        [/\$\{/, { token: "delimiter.bracket", next: "@tplExpr" }],
        [/`/, { token: "string.quote", bracket: "@close", next: "@pop" }],
      ],
      tplExpr: [[/\}/, { token: "delimiter.bracket", next: "@pop" }], { include: "root" }],
    },
  });
}
