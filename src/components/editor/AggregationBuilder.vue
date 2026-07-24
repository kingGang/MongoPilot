<script setup lang="ts">
import { ref, computed, watch } from "vue";
import {
  NModal, NCard, NButton, NInput, NInputNumber, NSelect, NDropdown, NIcon,
  NScrollbar, NSpace, NTag, NTooltip, useMessage,
} from "naive-ui";
import {
  AddOutline, TrashOutline, ArrowUpOutline, ArrowDownOutline,
  EyeOutline, PlayOutline, LayersOutline,
} from "@vicons/ionicons5";
import { runQuery } from "@/api/query";

const props = defineProps<{
  show: boolean;
  connectionId: string;
  database: string;
  collection?: string;
}>();

const emit = defineEmits<{
  "update:show": [val: boolean];
  /** OK and Run —— 生成 + 执行 */
  run: [queryText: string];
  /** Just OK —— 生成贴到编辑器, 不执行 */
  insert: [queryText: string];
}>();

const msg = useMessage();

interface Stage {
  id: string;
  op: string;
  body: string;
  enabled: boolean;
}

let seq = 0;
const nid = () => `st${seq++}`;

/** 常用聚合阶段 + 骨架模板 */
const STAGE_TYPES: { op: string; template: string; desc: string }[] = [
  { op: "$match", template: "{\n  \n}", desc: "过滤文档" },
  { op: "$group", template: '{\n  _id: "$field",\n  count: { $sum: 1 }\n}', desc: "分组聚合" },
  { op: "$project", template: "{\n  \n}", desc: "字段投影" },
  { op: "$addFields", template: "{\n  \n}", desc: "新增字段" },
  { op: "$set", template: "{\n  \n}", desc: "新增/覆盖字段" },
  { op: "$unset", template: '"field"', desc: "移除字段" },
  { op: "$sort", template: "{\n  _id: -1\n}", desc: "排序" },
  { op: "$limit", template: "10", desc: "限制条数" },
  { op: "$skip", template: "0", desc: "跳过条数" },
  { op: "$unwind", template: '"$field"', desc: "展开数组" },
  {
    op: "$lookup",
    template: '{\n  from: "otherColl",\n  localField: "x",\n  foreignField: "_id",\n  as: "joined"\n}',
    desc: "关联查询 (JOIN)",
  },
  { op: "$count", template: '"total"', desc: "计数" },
  { op: "$sortByCount", template: '"$field"', desc: "按出现次数分组排序" },
  { op: "$sample", template: "{\n  size: 100\n}", desc: "随机抽样" },
  { op: "$replaceRoot", template: '{\n  newRoot: "$doc"\n}', desc: "替换根文档" },
  { op: "$facet", template: "{\n  \n}", desc: "多面聚合" },
  { op: "$out", template: '"targetColl"', desc: "写入集合 (覆盖)" },
  { op: "$merge", template: '{\n  into: "targetColl"\n}', desc: "合并写入集合" },
];

const stageMenuOptions = STAGE_TYPES.map((s) => ({
  label: `${s.op}  —  ${s.desc}`,
  key: s.op,
}));

const WRITE_STAGES = new Set(["$out", "$merge"]);

// ---- 状态 ----
const coll = ref(props.collection ?? "");
const stages = ref<Stage[]>([]);
const previewLimit = ref(10);

const preview = ref<{
  stageId: string | null;
  docs: Record<string, unknown>[];
  count: number;
  ms: number;
  error: string;
  loading: boolean;
}>({ stageId: null, docs: [], count: 0, ms: 0, error: "", loading: false });

watch(
  () => props.show,
  (show) => {
    if (!show) return;
    coll.value = props.collection ?? "";
    stages.value = [makeStage("$match")];
    preview.value = { stageId: null, docs: [], count: 0, ms: 0, error: "", loading: false };
  },
);

function makeStage(op: string): Stage {
  const t = STAGE_TYPES.find((s) => s.op === op);
  return { id: nid(), op, body: t?.template ?? "{\n  \n}", enabled: true };
}

function addStage(op: string) {
  stages.value.push(makeStage(op));
}

function removeStage(id: string) {
  stages.value = stages.value.filter((s) => s.id !== id);
  if (preview.value.stageId === id) preview.value.stageId = null;
}

function moveStage(id: string, dir: -1 | 1) {
  const i = stages.value.indexOf(stages.value.find((s) => s.id === id)!);
  const j = i + dir;
  if (i < 0 || j < 0 || j >= stages.value.length) return;
  const arr = stages.value.slice();
  [arr[i], arr[j]] = [arr[j], arr[i]];
  stages.value = arr;
}

function changeOp(stage: Stage, op: string) {
  stage.op = op;
  const t = STAGE_TYPES.find((s) => s.op === op);
  // 仅当当前 body 还是空/模板时才替换, 避免覆盖用户已写的内容
  if (t) stage.body = t.template;
}

/** 集合名含 . 时用 getCollection() */
const collRef = computed(() =>
  coll.value.includes(".") ? `db.getCollection("${coll.value}")` : `db.${coll.value}`,
);

/** 把一组 stage 渲染成 aggregate([...]) 文本 */
function buildText(list: Stage[], extraLimit?: number): string {
  const parts = list.map((s) => `  { ${s.op}: ${s.body.trim()} }`);
  if (extraLimit != null) parts.push(`  { $limit: ${extraLimit} }`);
  return `${collRef.value}.aggregate([\n${parts.join(",\n")}\n])`;
}

const enabledStages = computed(() => stages.value.filter((s) => s.enabled && s.body.trim()));

const generatedText = computed(() => buildText(enabledStages.value));

const stageMenuRender = () => stageMenuOptions;

// ---- 预览 ----
async function runPreview(uptoStageId: string | null) {
  if (!coll.value.trim()) {
    msg.warning("请先填写集合名");
    return;
  }
  // 取到目标 stage (含) 为止的启用阶段; uptoStageId 为 null = 整段
  let list = enabledStages.value;
  if (uptoStageId) {
    const idx = stages.value.findIndex((s) => s.id === uptoStageId);
    const allowed = new Set(stages.value.slice(0, idx + 1).map((s) => s.id));
    list = enabledStages.value.filter((s) => allowed.has(s.id));
  }
  if (list.length === 0) {
    msg.warning("没有可预览的阶段");
    return;
  }
  const hasWrite = list.some((s) => WRITE_STAGES.has(s.op));
  // 写入阶段 ($out/$merge) 不追加 $limit 也不试跑, 避免真写库
  const text = hasWrite ? buildText(list) : buildText(list, previewLimit.value);
  if (hasWrite) {
    preview.value = {
      stageId: uptoStageId,
      docs: [],
      count: 0,
      ms: 0,
      error: "包含 $out / $merge 写入阶段，预览已跳过（避免真实写库）。请用「生成并运行」执行。",
      loading: false,
    };
    return;
  }

  preview.value.loading = true;
  preview.value.stageId = uptoStageId;
  preview.value.error = "";
  try {
    const res = await runQuery(
      props.connectionId,
      props.database,
      text,
      0,
      previewLimit.value,
    );
    preview.value.docs = res.documents;
    preview.value.count = res.count;
    preview.value.ms = res.executionTimeMs;
  } catch (e) {
    preview.value.docs = [];
    preview.value.count = 0;
    preview.value.error = String(e);
  } finally {
    preview.value.loading = false;
  }
}

const previewJson = computed(() => {
  if (preview.value.docs.length === 0) return "";
  return JSON.stringify(preview.value.docs, null, 2);
});

// ---- 提交 ----
function onRun() {
  if (!validate()) return;
  emit("run", generatedText.value);
  emit("update:show", false);
}
function onInsert() {
  if (!validate()) return;
  emit("insert", generatedText.value);
  emit("update:show", false);
}
function validate(): boolean {
  if (!coll.value.trim()) {
    msg.warning("请先填写集合名");
    return false;
  }
  if (enabledStages.value.length === 0) {
    msg.warning("至少需要一个启用且非空的阶段");
    return false;
  }
  return true;
}
function onClose() {
  emit("update:show", false);
}
</script>

<template>
  <n-modal
    :show="show"
    :trap-focus="false"
    :mask-closable="false"
    @update:show="emit('update:show', $event)"
  >
    <n-card
      :bordered="false"
      role="dialog"
      style="width: 860px; max-width: 94vw"
      @close="onClose"
    >
      <template #header>
        <div class="agg-header">
          <n-icon :size="18" style="margin-right: 6px"><LayersOutline /></n-icon>
          聚合管道构建器
        </div>
      </template>

      <!-- 集合 / 库 -->
      <div class="agg-target">
        <span class="agg-label">数据库</span>
        <n-tag size="small" type="info">{{ database }}</n-tag>
        <span class="agg-label" style="margin-left: 16px">集合</span>
        <n-input v-model:value="coll" size="small" placeholder="集合名" style="width: 220px" />
      </div>

      <div class="agg-body">
        <!-- 左: 阶段列表 -->
        <div class="agg-stages">
          <n-scrollbar style="max-height: 420px">
            <div v-for="(stage, i) in stages" :key="stage.id" class="stage-card" :class="{ disabled: !stage.enabled }">
              <div class="stage-head">
                <n-select
                  :value="stage.op"
                  :options="STAGE_TYPES.map((s) => ({ label: s.op, value: s.op }))"
                  size="tiny"
                  style="width: 150px"
                  @update:value="(op: string) => changeOp(stage, op)"
                />
                <span class="stage-idx">#{{ i + 1 }}</span>
                <div class="stage-actions">
                  <n-tooltip trigger="hover"><template #trigger>
                    <n-button size="tiny" quaternary :disabled="i === 0" @click="moveStage(stage.id, -1)">
                      <template #icon><n-icon><ArrowUpOutline /></n-icon></template>
                    </n-button>
                  </template>上移</n-tooltip>
                  <n-tooltip trigger="hover"><template #trigger>
                    <n-button size="tiny" quaternary :disabled="i === stages.length - 1" @click="moveStage(stage.id, 1)">
                      <template #icon><n-icon><ArrowDownOutline /></n-icon></template>
                    </n-button>
                  </template>下移</n-tooltip>
                  <n-tooltip trigger="hover"><template #trigger>
                    <n-button size="tiny" quaternary :type="stage.enabled ? 'default' : 'warning'" @click="stage.enabled = !stage.enabled">
                      <template #icon><n-icon><EyeOutline /></n-icon></template>
                    </n-button>
                  </template>{{ stage.enabled ? "禁用此阶段" : "启用此阶段" }}</n-tooltip>
                  <n-tooltip trigger="hover"><template #trigger>
                    <n-button size="tiny" quaternary type="primary" @click="runPreview(stage.id)">
                      <template #icon><n-icon><PlayOutline /></n-icon></template>
                    </n-button>
                  </template>预览到此阶段</n-tooltip>
                  <n-tooltip trigger="hover"><template #trigger>
                    <n-button size="tiny" quaternary type="error" @click="removeStage(stage.id)">
                      <template #icon><n-icon><TrashOutline /></n-icon></template>
                    </n-button>
                  </template>删除</n-tooltip>
                </div>
              </div>
              <n-input
                v-model:value="stage.body"
                type="textarea"
                size="small"
                :autosize="{ minRows: 2, maxRows: 8 }"
                class="stage-body"
                placeholder="阶段内容 (JSON / mongosh 表达式)"
              />
              <div v-if="preview.stageId === stage.id" class="stage-preview-flag">
                ▲ 当前预览到此
              </div>
            </div>
          </n-scrollbar>

          <n-dropdown trigger="click" :options="stageMenuRender()" @select="addStage">
            <n-button dashed size="small" style="width: 100%; margin-top: 8px">
              <template #icon><n-icon><AddOutline /></n-icon></template>
              添加阶段
            </n-button>
          </n-dropdown>
        </div>

        <!-- 右: 预览 -->
        <div class="agg-preview">
          <div class="preview-toolbar">
            <span class="preview-title">预览</span>
            <div style="flex: 1" />
            <span class="preview-limit-label">样本</span>
            <n-input-number v-model:value="previewLimit" size="tiny" :min="1" :max="200" style="width: 92px" />
            <n-button size="tiny" type="primary" :loading="preview.loading" @click="runPreview(null)">
              预览整段
            </n-button>
          </div>
          <div class="preview-meta">
            <template v-if="preview.loading">执行中...</template>
            <template v-else-if="preview.error" />
            <template v-else-if="preview.docs.length || preview.count">
              返回 {{ preview.count }} 条 · {{ preview.ms }} ms
            </template>
            <template v-else>点「预览到此阶段」或「预览整段」查看样本输出</template>
          </div>
          <div v-if="preview.error" class="preview-error">{{ preview.error }}</div>
          <n-scrollbar v-else class="preview-out">
            <pre v-if="previewJson">{{ previewJson }}</pre>
            <div v-else class="preview-empty">（无输出）</div>
          </n-scrollbar>
        </div>
      </div>

      <!-- 生成的语句 -->
      <div class="agg-generated">
        <div class="gen-label">生成的语句</div>
        <pre class="gen-text">{{ generatedText }}</pre>
      </div>

      <template #action>
        <n-space justify="end">
          <n-button size="small" @click="onClose">取消</n-button>
          <n-button size="small" @click="onInsert">插入编辑器</n-button>
          <n-button size="small" type="primary" @click="onRun">
            <template #icon><n-icon><PlayOutline /></n-icon></template>
            生成并运行
          </n-button>
        </n-space>
      </template>
    </n-card>
  </n-modal>
</template>

<style scoped>
.agg-header { display: flex; align-items: center; font-weight: 600; }
.agg-target { display: flex; align-items: center; gap: 8px; margin-bottom: 12px; }
.agg-label { font-size: 12px; color: #666; }
.agg-body { display: flex; gap: 12px; }
.agg-stages { flex: 1; min-width: 0; }
.agg-preview { width: 340px; flex-shrink: 0; display: flex; flex-direction: column; border: 1px solid #e5e5e5; border-radius: 6px; overflow: hidden; }

.stage-card { border: 1px solid #e0e0e0; border-radius: 6px; padding: 8px; margin-bottom: 8px; background: #fafafa; }
.stage-card.disabled { opacity: 0.5; }
.stage-head { display: flex; align-items: center; gap: 6px; margin-bottom: 6px; }
.stage-idx { font-size: 11px; color: #999; }
.stage-actions { display: flex; gap: 2px; margin-left: auto; }
.stage-body :deep(textarea) { font-family: Consolas, Monaco, monospace; font-size: 12px; }
.stage-preview-flag { font-size: 11px; color: #2080f0; margin-top: 4px; }

.preview-toolbar { display: flex; align-items: center; gap: 6px; padding: 6px 8px; background: #f2f2f2; border-bottom: 1px solid #e5e5e5; }
.preview-title { font-weight: 600; font-size: 13px; }
.preview-limit-label { font-size: 12px; color: #666; }
.preview-meta { font-size: 12px; color: #888; padding: 6px 8px; }
.preview-error { color: #d03050; font-size: 12px; padding: 8px; white-space: pre-wrap; word-break: break-all; }
.preview-out { flex: 1; padding: 0 8px 8px; }
.preview-out pre { margin: 0; font-family: Consolas, Monaco, monospace; font-size: 12px; white-space: pre-wrap; word-break: break-word; }
.preview-empty { color: #999; font-size: 12px; padding: 8px; }

.agg-generated { margin-top: 12px; }
.gen-label { font-size: 12px; color: #666; margin-bottom: 4px; }
.gen-text { margin: 0; max-height: 120px; overflow: auto; background: #1e1e1e; color: #d4d4d4; padding: 8px 10px; border-radius: 4px; font-family: Consolas, Monaco, monospace; font-size: 12px; white-space: pre; }
</style>
