#!/usr/bin/env node
/**
 * 发版版本号一键修改。
 *
 * 把版本号同步到三处需要提交的文件:
 *   - package.json
 *   - src-tauri/tauri.conf.json   (运行时 StatusBar 显示的版本就读这个)
 *   - src-tauri/Cargo.toml
 * 然后跑一次 cargo check 让 Cargo.lock 同步 (Cargo.lock 被 gitignore, 但编译要同步)。
 *
 * 用法:
 *   pnpm bump 0.1.5
 *   node scripts/bump-version.mjs 0.1.5
 */
import { readFileSync, writeFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { execSync } from "node:child_process";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+$/.test(version)) {
  console.error("用法: pnpm bump <x.y.z>    例如: pnpm bump 0.1.5");
  process.exit(1);
}

/** 读文件 -> transform -> 写回; 内容没变时给出警告 */
function patch(relPath, transform) {
  const path = resolve(root, relPath);
  const before = readFileSync(path, "utf8");
  const after = transform(before);
  if (before === after) {
    console.warn(`!  ${relPath} 未改动 (可能已是 ${version}, 或没匹配到版本号字段)`);
    return;
  }
  writeFileSync(path, after);
  console.log(`✓  ${relPath}`);
}

// 1. package.json —— 顶层 "version": "x.y.z"
patch("package.json", (s) =>
  s.replace(/("version"\s*:\s*")[^"]+(")/, `$1${version}$2`),
);

// 2. src-tauri/tauri.conf.json —— 顶层 "version": "x.y.z" (文件里第一个 version 字段)
patch("src-tauri/tauri.conf.json", (s) =>
  s.replace(/("version"\s*:\s*")[^"]+(")/, `$1${version}$2`),
);

// 3. src-tauri/Cargo.toml —— [package] 段里行首的 version = "x.y.z" (只改第一处)
patch("src-tauri/Cargo.toml", (s) =>
  s.replace(/^version\s*=\s*"[^"]+"/m, `version = "${version}"`),
);

// 4. cargo check 同步 Cargo.lock
console.log("\n运行 cargo check 同步 Cargo.lock ...");
try {
  execSync("cargo check", { cwd: resolve(root, "src-tauri"), stdio: "inherit" });
} catch {
  console.error("\n!  cargo check 失败 —— 修复后再发版");
  process.exit(1);
}

console.log(`\n版本号已全部更新为 ${version}。接下来手动执行:`);
console.log(`  git add package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json`);
console.log(`  git commit -m "release: v${version}"`);
console.log(`  git push origin main`);
console.log(`  git tag -a v${version} -m "v${version}" && git push origin v${version}`);
