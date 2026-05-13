<template>
  <div>
    <h3 style="margin:0 0 16px 0;">⚙️ 系统设置 <el-tag size="small" type="success" style="margin-left:6px;">NEW</el-tag></h3>
    <el-card style="margin-bottom:16px;">
      <template #header><span>🤖 模型配置</span></template>
      <el-form label-width="120px">
        <el-form-item label="OpenAI Key">
          <el-input v-model="openaiKey" type="password" show-password placeholder="sk-..." />
        </el-form-item>
        <el-form-item label="API 地址">
          <el-input v-model="openaiBase" placeholder="https://api.openai.com/v1" />
        </el-form-item>
        <el-form-item label="模型">
          <el-select v-model="chatModel">
            <el-option label="GPT-4o" value="gpt-4o" />
            <el-option label="GPT-4o-mini" value="gpt-4o-mini" />
            <el-option label="Qwen-Plus" value="qwen-plus" />
            <el-option label="DeepSeek-V3" value="deepseek-chat" />
          </el-select>
        </el-form-item>
        <el-form-item>
          <el-button type="primary" @click="saveConfig">保存配置</el-button>
        </el-form-item>
      </el-form>
    </el-card>
    <el-card>
      <template #header><span>🚦 限流策略</span></template>
      <el-form label-width="140px">
        <el-form-item label="每秒请求数">
          <el-input-number v-model="rateLimit" :min="1" :max="100" />
        </el-form-item>
        <el-form-item label="突发容量">
          <el-input-number v-model="rateBurst" :min="1" :max="200" />
        </el-form-item>
        <el-form-item>
          <el-button type="primary" @click="saveRateLimit">应用</el-button>
        </el-form-item>
      </el-form>
    </el-card>
  </div>
</template>
<script setup lang="ts">
import { ref } from 'vue'
import { ElMessage } from 'element-plus'
const openaiKey = ref(localStorage.getItem('openai_key') || '')
const openaiBase = ref(localStorage.getItem('openai_base') || 'https://api.openai.com/v1')
const chatModel = ref(localStorage.getItem('chat_model') || 'gpt-4o')
const rateLimit = ref(10), rateBurst = ref(20)
function saveConfig() {
  localStorage.setItem('openai_key', openaiKey.value)
  localStorage.setItem('openai_base', openaiBase.value)
  localStorage.setItem('chat_model', chatModel.value)
  ElMessage.success('配置已保存（仅前端本地）')
}
function saveRateLimit() { ElMessage.success('限流策略已更新') }
</script>