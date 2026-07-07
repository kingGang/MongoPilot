import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import type { InvokeArgs, InvokeOptions } from "@tauri-apps/api/core";

/** invoke 包装: 命令失败时把「命令名 + 参数摘要 + 报错」追加到本地
 *  %APPDATA%/com.mongopilot.app/error.log —— 排查偶现问题用,
 *  弹窗消失后仍能查到当时的报错. 日志写入本身失败则静默. */
export async function invoke<T>(
  cmd: string,
  args?: InvokeArgs,
  options?: InvokeOptions,
): Promise<T> {
  try {
    return await tauriInvoke<T>(cmd, args, options);
  } catch (e) {
    logClientError(`invoke ${cmd} 失败: ${String(e)}`);
    throw e;
  }
}

/** 直接写一条错误日志 (全局异常钩子等场景) */
export function logClientError(message: string) {
  tauriInvoke("log_client_error", { message: message.slice(0, 4000) }).catch(() => {});
}
