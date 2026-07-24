<script setup lang="ts">
import { reactive, ref, toRaw } from "vue";
import {
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NSelect,
  NSwitch,
  NButton,
  NTabs,
  NTabPane,
  NSpace,
  NCheckbox,
  useMessage,
} from "naive-ui";
import type { ConnectionConfig } from "@/types/connection";
import { exportUri } from "@/api/connection";
import UriImport from "./UriImport.vue";

const props = defineProps<{
  config: ConnectionConfig;
}>();

const emit = defineEmits<{
  save: [config: ConnectionConfig];
  test: [config: ConnectionConfig];
  cancel: [];
}>();

const form = reactive<ConnectionConfig>({ ...props.config });

const connTypeOptions = [
  { label: "单机 (Standalone)", value: "standalone" },
  { label: "副本集 (Replica Set)", value: "replicaset" },
  { label: "分片集群 (Sharded)", value: "sharded" },
];

const authTypeOptions = [
  { label: "无认证", value: "none" },
  { label: "用户名 / 密码", value: "password" },
  { label: "X.509 证书", value: "x509" },
  { label: "LDAP", value: "ldap" },
];

const sshAuthOptions = [
  { label: "密码", value: "password" },
  { label: "私钥", value: "privateKey" },
];

function handleSave() {
  emit("save", { ...toRaw(form) });
}

function handleTest() {
  emit("test", { ...toRaw(form) });
}

function handleUriImport(imported: ConnectionConfig) {
  Object.assign(form, imported);
  form.id = props.config.id;
}

const message = useMessage();
const exportedUri = ref("");

async function handleExportUri() {
  try {
    exportedUri.value = await exportUri({ ...toRaw(form) });
  } catch (e) {
    message.error(`导出失败: ${e}`);
  }
}

function handleCopyUri() {
  if (exportedUri.value) {
    navigator.clipboard.writeText(exportedUri.value);
    message.success("URI 已复制到剪贴板");
  }
}
</script>

<template>
  <div class="connection-form">
    <n-tabs type="line">
      <n-tab-pane name="general" tab="基本">
        <n-form label-placement="left" label-width="120">
          <n-form-item label="名称">
            <n-input v-model:value="form.name" placeholder="My Server" />
          </n-form-item>
          <n-form-item label="类型">
            <n-select v-model:value="form.connType" :options="connTypeOptions" />
          </n-form-item>
          <n-form-item label="主机">
            <n-input v-model:value="form.host" placeholder="localhost" />
          </n-form-item>
          <n-form-item label="端口">
            <n-input-number v-model:value="form.port" :min="1" :max="65535" />
          </n-form-item>
          <n-form-item label="SRV">
            <n-switch v-model:value="form.srv" />
          </n-form-item>
          <n-form-item v-if="form.connType === 'replicaset'" label="副本集">
            <n-input v-model:value="form.replicaSet" placeholder="rs0" />
          </n-form-item>
          <n-form-item label="默认数据库">
            <n-input v-model:value="form.defaultDb" placeholder="(可选)" />
          </n-form-item>
          <n-form-item label="分组">
            <n-input v-model:value="form.groupPath" placeholder="例如：生产环境" />
          </n-form-item>
          <n-form-item label=" ">
            <n-checkbox v-model:checked="form.readOnly">
              设为只读连接（禁止写入/修改/删除操作）
            </n-checkbox>
          </n-form-item>
        </n-form>
      </n-tab-pane>

      <n-tab-pane name="auth" tab="认证">
        <n-form label-placement="left" label-width="120">
          <n-form-item label="认证方式">
            <n-select v-model:value="form.authType" :options="authTypeOptions" />
          </n-form-item>
          <!-- 密码 / LDAP: 用户名 + 密码 -->
          <template v-if="form.authType === 'password' || form.authType === 'ldap'">
            <n-form-item label="用户名">
              <n-input v-model:value="form.username" />
            </n-form-item>
            <n-form-item label="密码">
              <n-input
                v-model:value="form.password"
                type="password"
                show-password-on="click"
              />
            </n-form-item>
          </template>
          <!-- authSource 只对密码认证有意义; ldap/x509 固定走 $external -->
          <n-form-item v-if="form.authType === 'password'" label="认证数据库">
            <n-input v-model:value="form.authDb" placeholder="admin" />
          </n-form-item>
          <!-- X.509: 用户名可选 (留空从证书 subject 取) -->
          <template v-if="form.authType === 'x509'">
            <n-form-item label="用户名">
              <n-input v-model:value="form.username" placeholder="留空则从证书 subject 自动获取" />
            </n-form-item>
          </template>
          <n-form-item v-if="form.authType === 'x509'" label=" " :show-feedback="false">
            <span class="auth-hint">
              X.509 以「TLS/SSL」页里的<strong>客户端证书文件</strong>作为身份，authSource 固定 $external。请在 TLS 页填好证书文件并启用 TLS。
            </span>
          </n-form-item>
          <n-form-item v-if="form.authType === 'ldap'" label=" " :show-feedback="false">
            <span class="auth-hint">
              LDAP 走 PLAIN 机制、authSource $external。用户名/密码为明文，建议同时启用 TLS 加密传输。
            </span>
          </n-form-item>
        </n-form>
      </n-tab-pane>

      <n-tab-pane name="tls" tab="TLS/SSL">
        <n-form label-placement="left" label-width="120">
          <n-form-item label="启用 TLS">
            <n-switch v-model:value="form.tls" />
          </n-form-item>
          <template v-if="form.tls">
            <n-form-item label="CA 文件">
              <n-input v-model:value="form.tlsCaFile" placeholder="/path/to/ca.pem" />
            </n-form-item>
            <n-form-item label="证书文件">
              <n-input v-model:value="form.tlsCertFile" placeholder="/path/to/cert.pem" />
            </n-form-item>
            <n-form-item label="密钥文件">
              <n-input v-model:value="form.tlsKeyFile" placeholder="/path/to/key.pem" />
            </n-form-item>
            <n-form-item label="允许无效证书">
              <n-switch v-model:value="form.tlsAllowInvalid" />
            </n-form-item>
          </template>
        </n-form>
      </n-tab-pane>

      <n-tab-pane name="ssh" tab="SSH 隧道">
        <n-form label-placement="left" label-width="120">
          <n-form-item label="启用 SSH">
            <n-switch v-model:value="form.sshEnabled" />
          </n-form-item>
          <template v-if="form.sshEnabled">
            <n-form-item label="SSH 主机">
              <n-input v-model:value="form.sshHost" />
            </n-form-item>
            <n-form-item label="SSH 端口">
              <n-input-number v-model:value="form.sshPort" :min="1" :max="65535" />
            </n-form-item>
            <n-form-item label="SSH 用户名">
              <n-input v-model:value="form.sshUsername" />
            </n-form-item>
            <n-form-item label="认证方式">
              <n-select v-model:value="form.sshAuthType" :options="sshAuthOptions" />
            </n-form-item>
            <template v-if="form.sshAuthType === 'password'">
              <n-form-item label="SSH 密码">
                <n-input
                  v-model:value="form.sshPassword"
                  type="password"
                  show-password-on="click"
                />
              </n-form-item>
            </template>
            <template v-if="form.sshAuthType === 'privateKey'">
              <n-form-item label="私钥路径">
                <n-input
                  v-model:value="form.sshPrivateKey"
                  placeholder="例如：C:\Users\you\.ssh\id_rsa"
                />
              </n-form-item>
              <n-form-item label="私钥口令">
                <n-input
                  v-model:value="form.sshPassphrase"
                  type="password"
                  show-password-on="click"
                />
              </n-form-item>
            </template>
          </template>
        </n-form>
      </n-tab-pane>

      <n-tab-pane name="uri" tab="URI">
        <n-space vertical :size="16">
          <div>
            <h4 style="margin: 0 0 8px">导入 URI</h4>
            <UriImport @import="handleUriImport" />
          </div>
          <div>
            <h4 style="margin: 0 0 8px">导出 URI</h4>
            <n-space vertical>
              <n-button size="small" type="primary" @click="handleExportUri">
                生成 URI
              </n-button>
              <template v-if="exportedUri">
                <n-input
                  :value="exportedUri"
                  type="textarea"
                  :rows="3"
                  readonly
                />
                <n-button size="small" @click="handleCopyUri">
                  复制到剪贴板
                </n-button>
              </template>
            </n-space>
          </div>
        </n-space>
      </n-tab-pane>
    </n-tabs>

    <div class="form-actions">
      <n-space>
        <n-button @click="$emit('cancel')">取消</n-button>
        <n-button type="info" @click="handleTest">Test Connection</n-button>
        <n-button type="primary" @click="handleSave">Save</n-button>
      </n-space>
    </div>
  </div>
</template>

<style scoped>
.connection-form {
  padding: 16px;
}
.form-actions {
  margin-top: 16px;
  display: flex;
  justify-content: flex-end;
}
.auth-hint {
  font-size: 12px;
  color: #999;
  line-height: 1.5;
}
</style>
