<template>
  <div style="display:flex;gap:16px;height:calc(100vh - 110px);">
    <!-- 左侧：对话面板 -->
    <div style="flex:1;display:flex;flex-direction:column;background:#fff;border-radius:8px;border:1px solid #e2e8f0;">
      <div style="padding:12px 16px;border-bottom:1px solid #e2e8f0;font-weight:600;color:#1e293b;">
        💬 对话
        <span style="font-size:12px;color:#94a3b8;margin-left:8px;">{{ store.messages.length }} 条消息</span>
      </div>
      <div ref="chatRef" style="flex:1;overflow-y:auto;padding:16px;display:flex;flex-direction:column;gap:12px;">
        <div v-for="(msg, i) in store.messages" :key="i" :style="msgStyle(msg.role)">
          <div style="font-size:11px;color:#94a3b8;margin-bottom:4px;">{{ msg.role === 'user' ? '🧑 你' : '🤖 Agent' }}</div>
          <div style="white-space:pre-wrap;font-size:14px;line-height:1.6;">{{ msg.content }}</div>
          <div v-if="msg.tool_calls?.length" style="margin-top:8px;">
            <div v-for="tc in msg.tool_calls" :key="tc.id"
              style="font-size:12px;background:#f1f5f9;border-radius:6px;padding:8px;margin-bottom:4px;border-left:3px solid #f59e0b;">
              <span style="font-weight:600;">🔧 {{ tc.tool }}</span>
              <span v-if="tc.duration_ms" style="color:#94a3b8;margin-left:8px;">{{ tc.duration_ms }}ms</span>
              <div v-if="tc.args" style="color:#64748b;margin-top:4px;font-family:monospace;font-size:11px;">{{ JSON.stringify(tc.args) }}</div>
            </div>
          </div>
        </div>
        <div v-if="store.streaming" style="text-align:center;color:#94a3b8;font-size:13px;">⏳ Agent 思考中...</div>
      </div>
      <div style="padding:12px 16px;border-top:1px solid #e2e8f0;display:flex;gap:8px;">
        <el-input
          v-model="input"
          placeholder="输入你的问题..."
          :disabled="store.streaming"
          @keyup.enter="sendMessage"
        />
        <el-button type="primary" :loading="store.streaming" @click="sendMessage" style="width:100px;">
          {{ store.streaming ? '发送中' : '发送' }}
        </el-button>
      </div>
    </div>

    <!-- 右侧：Tool 时间线 + Token 面板 -->
    <div style="width:380px;display:flex;flex-direction:column;gap:12px;">
      <!-- Tool 时间线 -->
      <div style="flex:1;background:#fff;border-radius:8px;border:1px solid #e2e8f0;display:flex;flex-direction:column;overflow:hidden;">
        <div style="padding:12px 16px;border-bottom:1px solid #e2e8f0;font-weight:600;color:#1e293b;">
          ⏱️ Tool 调用
          <span style="font-size:12px;color:#94a3b8;margin-left:8px;">{{ store.toolCalls.length }} 次</span>
        </div>
        <div style="flex:1;overflow-y:auto;padding:12px;">
          <div v-if="store.toolCalls.length === 0" style="text-align:center;color:#94a3b8;padding:40px 0;font-size:13px;">
            暂无工具调用
          </div>
          <div v-for="(tc, i) in store.toolCalls" :key="tc.id"
            style="padding:10px;border:1px solid #e2e8f0;border-radius:8px;margin-bottom:8px;"
            :style="{ borderLeftColor: tc.status === 'error' ? '#ef4444' : tc.status === 'done' ? '#10b981' : '#f59e0b', borderLeftWidth:'3px', borderLeftStyle:'solid' }">
            <div style="display:flex;justify-content:space-between;align-items:center;">
              <span style="font-weight:600;font-size:13px;">#{{ i+1 }} {{ tc.tool }}</span>
              <el-tag :type="tagType(tc.status)" size="small">{{ statusLabel(tc.status) }}</el-tag>
            </div>
            <div v-if="tc.args" style="margin-top:6px;font-size:11px;color:#64748b;font-family:monospace;background:#f8fafc;border-radius:4px;padding:6px;max-height:80px;overflow-y:auto;">
              {{ JSON.stringify(tc.args, null, 2) }}
            </div>
            <div v-if="tc.duration_ms" style="margin-top:4px;font-size:11px;color:#94a3b8;">
              ⏱ {{ tc.duration_ms }}ms
            </div>
            <div v-if="tc.result" style="margin-top:4px;font-size:11px;color:#475569;max-height:60px;overflow:hidden;text-overflow:ellipsis;">
              {{ tc.result.substring(0, 100) }}{{ tc.result.length > 100 ? '...' : '' }}
            </div>
          </div>
        </div>
      </div>

      <!-- Token / 成本面板 -->
      <div style="background:#fff;border-radius:8px;border:1px solid #e2e8f0;padding:12px 16px;">
        <div style="display:flex;justify-content:space-between;margin-bottom:8px;">
          <span style="font-weight:600;font-size:13px;color:#1e293b;">💰 Token 统计</span>
        </div>
        <div style="display:flex;gap:16px;font-size:13px;">
          <div><span style="color:#64748b;">Tokens:</span> <strong>{{ store.totalTokens.toLocaleString() }}</strong></div>
          <div><span style="color:#64748b;">费用:</span> <strong>${{ store.totalCost.toFixed(4) }}</strong></div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, nextTick } from 'vue'
import { useAgentStore } from '../../stores/agent'
import http from '../../api/client'

const store = useAgentStore()
const input = ref('')
const chatRef = ref<HTMLElement | null>(null)

function msgStyle(role: string) {
  return {
    background: role === 'user' ? '#eff6ff' : '#fafafa',
    borderRadius: '8px',
    padding: '10px 14px',
    maxWidth: '85%',
    alignSelf: role === 'user' ? 'flex-end' : 'flex-start',
  } as Record<string, string>
}

function tagType(status: string) {
  if (status === 'done') return 'success'
  if (status === 'error') return 'danger'
  if (status === 'running') return 'warning'
  return 'info'
}

function statusLabel(status: string) {
  const map: Record<string, string> = { pending: '等待', running: '执行中', done: '完成', error: '失败' }
  return map[status] || status
}

async function sendMessage() {
  const text = input.value.trim()
  if (!text || store.streaming) return
  input.value = ''
  store.addMessage({ role: 'user', content: text })
  store.streaming = true
  scrollToBottom()

  try {
      const resp = await fetch('/api/v2/chat', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        message: text,
        tools: true,
      }),
    })
    if (!resp.ok) throw new Error('请求失败')

    const reader = resp.body?.getReader()
    if (!reader) throw new Error('无法读取流')
    const decoder = new TextDecoder()
    let assistantMsg = ''

    while (true) {
      const { done, value } = await reader.read()
      if (done) break
      const chunk = decoder.decode(value, { stream: true })
      const lines = chunk.split('\n').filter(l => l.startsWith('data: '))
      for (const line of lines) {
        const data = JSON.parse(line.slice(6))
        if (data.type === 'text') {
          assistantMsg += data.content
          updateAssistantMessage(assistantMsg)
        } else if (data.type === 'tool_call') {
          store.addToolCall({
            id: data.id,
            tool: data.tool,
            args: data.args,
            status: 'running',
          })
        } else if (data.type === 'tool_result') {
          store.updateToolCall(data.id, { result: data.result, duration_ms: data.duration_ms, status: 'done' })
          if (data.tokens) store.recordTokens(data.tokens)
        }
      }
    }
    if (assistantMsg) store.addMessage({ role: 'assistant', content: assistantMsg })
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : '未知错误'
    store.addMessage({ role: 'assistant', content: `❌ ${msg}` })
  } finally {
    store.streaming = false
    scrollToBottom()
  }
}

function updateAssistantMessage(content: string) {
  const last = store.messages[store.messages.length - 1]
  if (last?.role === 'assistant') {
    last.content = content
  } else {
    store.addMessage({ role: 'assistant', content })
  }
}

async function scrollToBottom() {
  await nextTick()
  if (chatRef.value) chatRef.value.scrollTop = chatRef.value.scrollHeight
}
</script>
