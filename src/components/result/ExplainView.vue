<script setup lang="ts">
import { computed, ref } from "vue";
import { NIcon } from "naive-ui";
import {
  Leaf as StageIcon,
  HelpCircleOutline as HelpIcon,
  ArrowUp as ArrowIcon,
  InformationCircleOutline as InfoIcon,
} from "@vicons/ionicons5";

interface StageNode {
  stage: string;
  nReturned?: number;
  executionTimeMillisEstimate?: number;
  indexName?: string;
  isUnique?: boolean;
  isMultiKey?: boolean;
  direction?: string;
  keyPattern?: Record<string, unknown>;
  indexBounds?: Record<string, unknown>;
  inputStage?: StageNode;
  inputStages?: StageNode[];
  sortPattern?: Record<string, unknown>;
  memUsage?: number;
  memLimit?: number;
  [key: string]: unknown;
}

const props = defineProps<{
  explainResult: Record<string, unknown>;
}>();

function num(v: unknown): number {
  if (typeof v === "number") return v;
  if (typeof v === "string" && !isNaN(Number(v))) return Number(v);
  if (v && typeof v === "object") {
    const o = v as Record<string, unknown>;
    if (typeof o.$numberLong === "string") return Number(o.$numberLong);
    if (typeof o.$numberInt === "string") return Number(o.$numberInt);
    if (typeof o.$numberDouble === "string") return Number(o.$numberDouble);
  }
  return 0;
}

const executionStats = computed<Record<string, unknown> | null>(() => {
  const r = props.explainResult as Record<string, unknown>;
  return (r.executionStats as Record<string, unknown>) || null;
});

const docsReturned = computed(() => num(executionStats.value?.nReturned));
const keysExamined = computed(() => num(executionStats.value?.totalKeysExamined));
const docsExamined = computed(() => num(executionStats.value?.totalDocsExamined));
const execTimeMs = computed(() => num(executionStats.value?.executionTimeMillis));

/**
 * 把 inputStage / inputStages 树压平为"从 root 到 leaf"的列表, 用于纵向渲染.
 */
function flattenStages(root: StageNode | null): StageNode[] {
  if (!root) return [];
  const out: StageNode[] = [];
  let cur: StageNode | undefined = root;
  while (cur) {
    out.push(cur);
    if (cur.inputStage) {
      cur = cur.inputStage;
    } else if (Array.isArray(cur.inputStages) && cur.inputStages.length > 0) {
      // 多子阶段 (如 OR): 只跟第一个, 并在该节点上标注 "多分支"
      cur = cur.inputStages[0];
    } else {
      cur = undefined;
    }
  }
  return out;
}

const stages = computed<StageNode[]>(() => {
  const root = (executionStats.value?.executionStages as StageNode) || null;
  return flattenStages(root);
});

const sortedInMemory = computed<boolean>(() => {
  // SORT stage 存在且不是 SORT_MERGE / SORT_KEY_GENERATOR 之外, 且 memUsage > 0
  return stages.value.some(
    (s) => s.stage === "SORT" && num(s.memUsage) >= 0 && !s.inputStage?.indexName,
  );
});

const winningIndex = computed<string | null>(() => {
  const ix = stages.value.find((s) => s.stage === "IXSCAN");
  return ix?.indexName ?? null;
});

function stageExecTime(s: StageNode): string {
  return num(s.executionTimeMillisEstimate).toFixed(3);
}

// 展开的 stage details 索引
const expandedDetails = ref<Set<number>>(new Set());
function toggleDetails(i: number) {
  if (expandedDetails.value.has(i)) expandedDetails.value.delete(i);
  else expandedDetails.value.add(i);
  expandedDetails.value = new Set(expandedDetails.value);
}

function stageDetails(s: StageNode): [string, string][] {
  const rows: [string, string][] = [];
  const skip = new Set([
    "stage", "nReturned", "executionTimeMillisEstimate",
    "inputStage", "inputStages",
    "indexName", "isUnique", "direction",
    "works", "advanced", "needTime", "needYield",
    "saveState", "restoreState", "isEOF",
  ]);
  // 先显示一些常用字段
  const preferred = [
    "keyPattern", "indexBounds", "isMultiKey", "multiKeyPaths",
    "isSparse", "isPartial", "indexVersion",
    "sortPattern", "memUsage", "memLimit", "usedDisk",
    "docsExamined", "alreadyHasObj",
  ];
  for (const k of preferred) {
    if (k in s && !skip.has(k)) {
      rows.push([k, fmt((s as Record<string, unknown>)[k])]);
    }
  }
  // 剩余字段
  for (const [k, v] of Object.entries(s)) {
    if (skip.has(k) || preferred.includes(k)) continue;
    rows.push([k, fmt(v)]);
  }
  return rows;
}

function fmt(v: unknown): string {
  if (v === null || v === undefined) return "—";
  if (typeof v === "boolean") return String(v);
  if (typeof v === "number" || typeof v === "string") return String(v);
  try {
    return JSON.stringify(v);
  } catch {
    return String(v);
  }
}

// IXSCAN 特殊展示字段
function isIxscan(s: StageNode) { return s.stage === "IXSCAN"; }
</script>

<template>
  <div class="explain-view">
    <!-- 顶部汇总 -->
    <div class="section-header">Query Execution Statistics of the Winning Plan</div>
    <div class="summary-grid">
      <div class="summary-col">
        <div class="summary-row">
          <n-icon class="info-icon"><InfoIcon /></n-icon>
          <span class="label">Documents Returned:</span>
          <span class="badge green">{{ docsReturned }}</span>
        </div>
        <div class="summary-row">
          <n-icon class="info-icon"><InfoIcon /></n-icon>
          <span class="label">Index Keys Examined:</span>
          <span class="value">{{ keysExamined }}</span>
        </div>
        <div class="summary-row">
          <n-icon class="info-icon"><InfoIcon /></n-icon>
          <span class="label">Documents Examined:</span>
          <span class="value">{{ docsExamined }}</span>
        </div>
      </div>
      <div class="summary-col">
        <div class="summary-row">
          <n-icon class="info-icon"><InfoIcon /></n-icon>
          <span class="label">Actual Query Execution Time (ms):</span>
          <span class="badge blue">{{ execTimeMs }} ms</span>
        </div>
        <div class="summary-row">
          <n-icon class="info-icon"><InfoIcon /></n-icon>
          <span class="label">Sorted in Memory:</span>
          <span class="value">{{ sortedInMemory }}</span>
        </div>
        <div class="summary-row">
          <n-icon class="info-icon"><InfoIcon /></n-icon>
          <span class="label">Query used the following index:</span>
          <span v-if="winningIndex" class="index-tag">
            <span class="index-kind">REGULAR</span>
            <n-icon class="info-inline"><InfoIcon /></n-icon>
            <span class="index-name">{{ winningIndex }}</span>
          </span>
          <span v-else class="muted">—</span>
        </div>
      </div>
    </div>

    <!-- Stage 卡片堆 -->
    <div class="stages">
      <template v-for="(stage, i) in stages" :key="i">
        <div class="stage-card">
          <div class="stage-header">
            <n-icon class="stage-icon"><StageIcon /></n-icon>
            <span class="stage-name">STAGE: {{ stage.stage }}</span>
          </div>
          <div class="stage-metrics">
            <span class="label">nReturned:</span>
            <span class="badge green">{{ num(stage.nReturned) }}</span>
            <span class="label ml">Execution Time:</span>
            <span class="badge blue">{{ stageExecTime(stage) }} ms</span>
          </div>
          <!-- IXSCAN 特定行 -->
          <template v-if="isIxscan(stage)">
            <div class="stage-row">
              <span class="row-label">indexName</span>
              <span class="index-tag">
                <span class="index-kind">REGULAR</span>
                <n-icon class="info-inline"><InfoIcon /></n-icon>
                <span class="index-name">{{ stage.indexName }}</span>
              </span>
            </div>
            <div v-if="stage.isUnique !== undefined" class="stage-row">
              <span class="row-label">isUnique</span>
              <span class="row-value">{{ stage.isUnique }}</span>
            </div>
            <div v-if="stage.direction" class="stage-row">
              <span class="row-label">direction</span>
              <span class="row-value">{{ stage.direction }}</span>
            </div>
          </template>
          <!-- Details 折叠区 -->
          <div class="stage-details-toggle" @click="toggleDetails(i)">
            <n-icon class="q-icon"><HelpIcon /></n-icon>
            <span>Details</span>
            <span class="chevron">{{ expandedDetails.has(i) ? "▼" : "▶" }}</span>
          </div>
          <div v-if="expandedDetails.has(i)" class="stage-details">
            <table>
              <tr v-for="(row, rIdx) in stageDetails(stage)" :key="rIdx">
                <td class="k">{{ row[0] }}</td>
                <td class="v">{{ row[1] }}</td>
              </tr>
            </table>
          </div>
        </div>
        <div v-if="i < stages.length - 1" class="stage-arrow">
          <n-icon><ArrowIcon /></n-icon>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.explain-view {
  padding: 12px 16px 24px;
  overflow-y: auto;
  height: 100%;
  background: #fff;
  font-size: 13px;
  color: #333;
}

.section-header {
  padding: 10px 14px;
  background: #f5f5f5;
  border: 1px solid #e0e0e0;
  border-radius: 3px;
  font-weight: 600;
  color: #4a4a4a;
  margin-bottom: 12px;
}

.summary-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px 32px;
  padding: 8px 12px 16px;
  border-bottom: 1px solid #eee;
  margin-bottom: 18px;
}
.summary-col {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.summary-row {
  display: flex;
  align-items: center;
  gap: 6px;
  min-height: 26px;
}
.label {
  color: #666;
}
.value {
  color: #333;
  font-weight: 500;
}
.muted { color: #aaa; }
.info-icon {
  color: #bdbdbd;
  font-size: 16px;
}
.info-inline {
  color: #3875d7;
  font-size: 13px;
}

.badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 20px;
  height: 20px;
  padding: 0 8px;
  border-radius: 10px;
  color: #fff;
  font-size: 12px;
  font-weight: 600;
}
.badge.green { background: #18a058; }
.badge.blue { background: #3875d7; }

.index-tag {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 8px;
  border: 1px solid #e8a838;
  background: #fff9ea;
  border-radius: 12px;
  font-size: 12px;
}
.index-kind {
  color: #d18c00;
  font-weight: 700;
  font-size: 11px;
  letter-spacing: 0.3px;
}
.index-name {
  color: #333;
  font-family: "Fira Code", "Consolas", monospace;
}

.stages {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0;
}

.stage-card {
  width: min(420px, 100%);
  border: 1px solid #e0e0e0;
  border-radius: 3px;
  background: #fafafa;
  overflow: hidden;
  box-shadow: 0 1px 1px rgba(0, 0, 0, 0.02);
}
.stage-header {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 12px;
  background: #ebebeb;
  font-weight: 600;
  color: #333;
  border-bottom: 1px solid #dcdcdc;
}
.stage-icon { color: #18a058; font-size: 16px; }
.stage-name { font-family: "Fira Code", "Consolas", monospace; font-size: 13px; }

.stage-metrics {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  padding: 10px 12px;
  background: #fff;
  border-bottom: 1px solid #eee;
}
.stage-metrics .ml { margin-left: 16px; }

.stage-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 12px;
  background: #fff;
  border-bottom: 1px solid #eee;
}
.row-label {
  min-width: 90px;
  color: #888;
  font-size: 12px;
}
.row-value {
  font-weight: 500;
}

.stage-details-toggle {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 12px;
  cursor: pointer;
  color: #666;
  user-select: none;
  background: #f4f4f4;
}
.stage-details-toggle:hover { background: #eee; }
.q-icon { color: #b0b0b0; font-size: 14px; }
.chevron { margin-left: auto; color: #999; font-size: 10px; }

.stage-details {
  padding: 8px 12px;
  background: #fff;
  border-top: 1px solid #eee;
}
.stage-details table { width: 100%; border-collapse: collapse; }
.stage-details td {
  padding: 3px 6px;
  font-size: 12px;
  vertical-align: top;
  word-break: break-word;
}
.stage-details td.k {
  color: #888;
  white-space: nowrap;
  width: 160px;
  font-family: "Fira Code", "Consolas", monospace;
}
.stage-details td.v {
  color: #333;
  font-family: "Fira Code", "Consolas", monospace;
}

.stage-arrow {
  display: flex;
  justify-content: center;
  align-items: center;
  width: min(420px, 100%);
  height: 32px;
  color: #7fb3e8;
}
.stage-arrow :deep(svg) { width: 22px; height: 22px; }
</style>
