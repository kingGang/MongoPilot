# MongoPilot

一款基于 [Tauri 2](https://tauri.app) 和 Vue 3 构建的现代化跨平台 MongoDB 图形客户端。
连接管理、查询编写、数据浏览、AI 辅助——全部集成在一个轻量级原生应用中。

---

## 功能特性

- **连接管理** — 多连接保存，凭证加密存储；支持标准 URI、主机端口、副本集
- **SSH 隧道** — 通过 SSH 跳板机连接 MongoDB（支持密码与密钥认证）
- **查询编辑器** — Monaco Editor，支持 JavaScript / MongoDB Shell 语法、智能补全与查询历史
- **多视图结果** — Tree / Table / 原始 JSON 三种视图切换，大结果集分页
- **CRUD 操作** — 文档增删改查，内置编辑器
- **索引管理** — 查看、创建、删除集合索引
- **服务器监控** — 实时服务器状态、活跃操作、慢查询分析器
- **用户管理** — 列出、创建、删除指定数据库的 MongoDB 用户
- **导入 / 导出** — 查询结果导出为 JSON / CSV / XLSX；JSON / CSV 导入回集合
- **AI 辅助** — 自带 API Key（OpenAI 兼容接口），支持自然语言转查询、Schema 分析、索引建议
- **国际化** — 界面支持中文和英文

## 截图

> 截图待补充，欢迎提交 Pull Request。

---

## 安装

### 方式一：下载发行版（推荐）

1. 前往 [Releases](https://github.com/kingGang/MongoPilot/releases)
2. 下载适合你平台的安装包：
   - Windows：`.msi` 或 `.exe`（NSIS）
   - macOS：`.dmg`
   - Linux：`.AppImage` 或 `.deb`
3. 运行安装程序并启动 MongoPilot。

### 方式二：从源码构建

#### 依赖要求

| 工具 | 最低版本 | 安装地址 |
|------|----------|---------|
| Node.js | 18+ | https://nodejs.org |
| pnpm | 9+ | `npm i -g pnpm` |
| Rust（stable） | 1.70+ | https://rustup.rs |
| Tauri 系统依赖 | — | https://tauri.app/start/prerequisites/ |

```bash
git clone https://github.com/kingGang/MongoPilot.git
cd MongoPilot
./setup.sh          # 检查依赖并安装 Node 包
pnpm tauri dev      # 开发模式
# 或
pnpm tauri build    # 构建生产版本安装包
```

---

## 快速开始

1. 启动 MongoPilot。
2. 点击侧边栏的「新建连接」，填写 MongoDB 连接信息。
3. 点击「连接」。左侧数据库 / 集合树会自动展开。
4. 打开一个集合，在编辑器中使用 MongoDB Shell 语法编写查询，点击「运行」。
5. 在 Tree / Table / JSON 视图中浏览结果。
6. 如需启用 AI 辅助，前往「工具 → AI 设置」填入你的 OpenAI 兼容 API Key。

---

## 安全说明

- **凭证加密** — 保存的连接凭证（密码、SSH 密钥）使用 AES-256-GCM 加密，
  密钥在首次启动时自动生成并存放在操作系统的应用数据目录中，**永不离开你的电脑**。
- **AI API Key** — 保存在本地 SQLite 数据库中，仅会发送给你配置的 AI 服务提供方，
  不会传输给任何其他第三方。请自行妥善保管你的 API Key。

---

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面外壳 | Tauri 2 |
| 前端 | Vue 3 + TypeScript |
| UI 组件库 | Naive UI |
| 代码编辑器 | Monaco Editor |
| 图表 | ECharts + vue-echarts |
| 状态管理 | Pinia |
| 国际化 | vue-i18n |
| 后端 | Rust（stable） |
| MongoDB 驱动 | `mongodb` crate v3 |
| SSH 隧道 | `ssh2` crate |
| 加密 | `aes-gcm` crate（AES-256-GCM） |
| 本地数据库 | SQLite（via `sqlx`） |
| 构建工具 | Vite 6 |
| 测试框架 | Vitest + Playwright |

---

## 开发

```bash
pnpm tauri dev      # 启动开发模式（Vite :1420 + Tauri 窗口）
pnpm test           # 运行单元测试
pnpm lint           # ESLint 检查
pnpm format         # Prettier 格式化
```

---

## 贡献代码

欢迎贡献！提交 PR 前请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。

---

## 许可证

[MIT](LICENSE) — Copyright 2026 MongoPilot Contributors
