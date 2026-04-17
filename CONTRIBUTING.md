# 为 MongoPilot 贡献代码

感谢你对 MongoPilot 的关注！本文档涵盖从克隆仓库到合并 PR 所需的一切信息。

---

## 目录

1. [开发环境搭建](#开发环境搭建)
2. [项目结构](#项目结构)
3. [开发指引](#开发指引)
4. [代码风格](#代码风格)
5. [运行测试](#运行测试)
6. [分支与 PR 流程](#分支与-pr-流程)
7. [提交信息规范](#提交信息规范)
8. [提交 Issue](#提交-issue)

---

## 开发环境搭建

### 依赖要求

| 工具 | 最低版本 | 安装地址 |
|------|----------|---------|
| Node.js | 18+ | https://nodejs.org |
| pnpm | 9+ | `npm i -g pnpm` |
| Rust（stable 工具链） | 1.70+ | https://rustup.rs |
| Tauri 系统依赖 | — | https://tauri.app/start/prerequisites/ |

Tauri 的系统依赖因操作系统而异：
- **Windows** — WebView2（Windows 10+ 通常已预装）、VS Build Tools
- **macOS** — Xcode Command Line Tools（`xcode-select --install`）
- **Linux** — webkit2gtk、build-essential 等包（详见 Tauri 文档）

### 首次配置

```bash
git clone https://github.com/kingGang/MongoPilot.git
cd MongoPilot
./setup.sh          # 检查 Node / pnpm / rustc 版本，执行 pnpm install
pnpm tauri dev      # 启动 Vite :1420 开发服务器 + Tauri 原生窗口
```

---

## 项目结构

```
src/                    Vue 3 + TypeScript 前端
  api/                  Tauri invoke() 包装层——每个领域一个文件
  components/           按功能分组的 UI 组件
  stores/               Pinia 状态 store
  views/                顶层页面视图
  utils/                纯函数工具（BSON 格式化、Monaco 补全）
  types/                共享 TypeScript 接口和类型

src-tauri/              Rust 后端
  src/commands/         Tauri #[command] 处理函数
  src/connection/       ConnectionManager、SSH 隧道、配置
  src/query/            查询执行与导出
  src/ai/               AI 客户端与 Prompt 构造
  src/storage/          SQLite 仓储层（sqlx）
  src/crypto/           AES-256-GCM 密钥管理
  migrations/           SQLite 迁移文件
```

---

## 开发指引

### 前端（Vue / TypeScript）

- 所有 Tauri IPC 调用必须通过 `src/api/` 封装——**不要**在组件中直接调用 `invoke()`。
- 共享状态使用 `src/stores/` 下的 Pinia store；组件内部状态放在 `<script setup>` 中。
- 新增 UI 组件放在 `src/components/<feature>/` 目录下。
- 新增的用户可见字符串需要同时在 `en` 和 `zh` 两套语言文件中添加翻译。

### 后端（Rust）

- 新增的 Tauri 命令放在 `src-tauri/src/commands/`，并在 `lib.rs` 中注册。
- 业务逻辑不要写在命令处理函数里——处理函数应当只做薄薄的一层包装。
- 新增 SQLite 表需要添加编号的迁移文件到 `src-tauri/migrations/`。
- 日志中严禁输出敏感信息；合理使用 `tracing::debug!` / `tracing::error!`。

---

## 代码风格

### TypeScript / Vue

```bash
pnpm lint       # ESLint --fix（PR 前必须通过）
pnpm format     # Prettier --write（PR 前必须通过）
```

- 所有新组件使用 `<script setup lang="ts">`。
- 可组合逻辑优先使用 `const` 和箭头函数。
- 禁止使用 `any`——类型确实未知时使用 `unknown` 并显式缩窄。

### Rust

```bash
cd src-tauri
cargo fmt       # rustfmt 格式化（PR 前必须通过）
cargo clippy    # clippy 必须零警告（PR 前必须通过）
```

- 遵循 Rust 惯用写法；`unwrap()` 仅允许在测试中使用。
- 使用 `thiserror` 定义领域错误；通过 `?` 向上传递。
- 新增的异步函数使用 `async fn`，在 Tokio 运行时中执行。

---

## 运行测试

```bash
# 单元测试（Vitest，在 jsdom 中运行）
pnpm test

# 开发时的 watch 模式
pnpm test:watch

# 端到端测试（Playwright）
pnpm test:e2e

# Rust 测试
cd src-tauri && cargo test
```

测试文件位于 `tests/`（TypeScript）以及内嵌的 `#[cfg(test)]` 模块（Rust）。
新增功能至少应包含覆盖正向路径的单元测试。

---

## 分支与 PR 流程

1. **Fork** 仓库到自己的 GitHub 账号并 clone 下来。
2. 从 `main` 创建功能分支：
   ```bash
   git checkout -b feat/short-description
   ```
3. 提交聚焦的小颗粒度 commit（提交信息规范见下文）。
4. 推送前在本地确认所有检查通过：
   ```bash
   pnpm lint && pnpm format && pnpm test
   cd src-tauri && cargo fmt && cargo clippy && cargo test
   ```
5. 推送分支并向 `main` 发起 Pull Request。
6. 填写 PR 模板。用 `Closes #123` 关联相关 Issue。
7. Maintainer 会进行 Review，可能提出修改意见。请通过**追加新的 commit**
   回应 Review（Review 进行中不要强推）。

---

## 提交信息规范

使用 [Conventional Commits](https://www.conventionalcommits.org/zh-hans/) 规范：

```
<type>(<可选 scope>): <简短摘要>

[可选正文]

[可选页脚：Closes #issue]
```

常用 type：`feat`、`fix`、`refactor`、`docs`、`test`、`chore`、`perf`、`style`。

示例：
```
feat(ssh): 支持 SSH 私钥 Passphrase
fix(query): 防止重复执行时产生重复历史记录
docs: 更新 CONTRIBUTING 的环境搭建步骤
chore(deps): 升级 mongodb crate 到 3.1
```

- 摘要行不超过 72 个字符。
- 使用祈使语气（「add」而不是「added」或「adds」；中文直接写动词）。
- Issue 编号放在页脚而不是摘要中。

---

## 提交 Issue

请使用 GitHub 的 Issue 模板：

- **Bug 报告** — 包含复现步骤、期望行为 vs 实际行为、操作系统、应用版本。
- **功能请求** — 描述你想解决的问题以及建议的方案。

提交新 Issue 前请先搜索已有 Issue，避免重复。
