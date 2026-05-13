<template>
  <div>
    <h3 style="margin:0 0 16px 0;">🧪 评估中心 <el-tag size="small" type="success" style="margin-left:6px;">NEW</el-tag></h3>
    <el-card style="margin-bottom:16px;">
      <template #header><span>📝 单次评估</span></template>
      <el-input v-model="evalQuestion" placeholder="问题" style="margin-bottom:8px;" />
      <el-input v-model="evalAnswer" placeholder="回答" type="textarea" :rows="3" style="margin-bottom:8px;" />
      <el-input v-model="evalContext" placeholder="上下文（可选）" type="textarea" :rows="2" style="margin-bottom:8px;" />
      <el-button type="primary" @click="runEval" :loading="evalLoading">运行评估</el-button>
      <div v-if="evalResult" style="margin-top:12px;">
        <div v-for="(v,k) in evalResult" :key="k" style="display:flex;justify-content:space-between;padding:4px 0;border-bottom:1px solid #f1f5f9;">
          <span>{{ k }}</span><span :style="{color: v >= 0.7 ? '#10b981' : v >= 0.4 ? '#f59e0b' : '#ef4444', fontWeight:'bold'}">{{ (v*100).toFixed(0) }}%</span>
        </div>
      </div>
    </el-card>
    <el-card>
      <template #header><span>🔄 对比评估</span></template>
      <el-input v-model="compareQuestion" placeholder="问题" style="margin-bottom:8px;" />
      <el-input v-model="compareBase" placeholder="回答 A（基准）" type="textarea" :rows="3" style="margin-bottom:8px;" />
      <el-input v-model="compareNew" placeholder="回答 B（新）" type="textarea" :rows="3" style="margin-bottom:8px;" />
      <el-button type="primary" @click="runCompare" :loading="compareLoading">对比</el-button>
      <div v-if="compareResult" style="margin-top:12px;">
        <pre style="font-size:12px;">{{ JSON.stringify(compareResult, null, 2) }}</pre>
      </div>
    </el-card>
  </div>
</template>
<script setup lang="ts">
import { ref } from 'vue'
import http from '../../api/client'
import { ElMessage } from 'element-plus'
const evalQuestion = ref(''), evalAnswer = ref(''), evalContext = ref(''), evalResult = ref<any>(null), evalLoading = ref(false)
const compareQuestion = ref(''), compareBase = ref(''), compareNew = ref(''), compareResult = ref<any>(null), compareLoading = ref(false)
async function runEval() {
  if (!evalQuestion.value || !evalAnswer.value) { ElMessage.warning('请输入问题和回答'); return }
  evalLoading.value = true
  try {
    const { data } = await http.post('/v2/evaluate/run', { question: evalQuestion.value, answer: evalAnswer.value, context: evalContext.value })
    evalResult.value = data
  } catch (e: any) { ElMessage.error(e.message) } finally { evalLoading.value = false }
}
async function runCompare() {
  if (!compareQuestion.value || !compareBase.value || !compareNew.value) { ElMessage.warning('请填写完整'); return }
  compareLoading.value = true
  try {
    const { data } = await http.post('/v2/evaluate/compare', { question: compareQuestion.value, base_answer: compareBase.value, new_answer: compareNew.value })
    compareResult.value = data
  } catch (e: any) { ElMessage.error(e.message) } finally { compareLoading.value = false }
}
</script>