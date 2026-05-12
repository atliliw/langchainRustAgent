import { defineStore } from 'pinia'
import { ref } from 'vue'
import http from '../api/client'

export interface ChatMessage {
  role: 'user' | 'assistant' | 'tool'
  content: string
  tool_calls?: ToolCall[]
}

export interface ToolCall {
  id: string
  tool: string
  args: Record<string, unknown>
  result?: string
  duration_ms?: number
  tokens?: number
  status: 'pending' | 'running' | 'done' | 'error'
}

export interface Session {
  id: string
  title: string
  created_at: string
}

export const useAgentStore = defineStore('agent', () => {
  const sessions = ref<Session[]>([])
  const currentSessionId = ref<string | null>(null)
  const messages = ref<ChatMessage[]>([])
  const toolCalls = ref<ToolCall[]>([])
  const streaming = ref(false)
  const totalTokens = ref(0)
  const totalCost = ref(0)

  const PRICE_PER_1K = 0.002 // GPT-4o 近似单价

  async function loadSessions() {
    try {
      const { data } = await http.get('/agent/sessions')
      sessions.value = data || []
    } catch { sessions.value = [] }
  }

  function addMessage(msg: ChatMessage) {
    messages.value.push(msg)
  }

  function addToolCall(tc: ToolCall) {
    toolCalls.value.push(tc)
  }

  function updateToolCall(id: string, patch: Partial<ToolCall>) {
    const idx = toolCalls.value.findIndex(t => t.id === id)
    if (idx >= 0) Object.assign(toolCalls.value[idx], patch)
  }

  function recordTokens(tokens: number) {
    totalTokens.value += tokens
    totalCost.value += (tokens / 1000) * PRICE_PER_1K
  }

  function reset() {
    messages.value = []
    toolCalls.value = []
    totalTokens.value = 0
    totalCost.value = 0
    streaming.value = false
  }

  return {
    sessions, currentSessionId, messages, toolCalls,
    streaming, totalTokens, totalCost,
    loadSessions, addMessage, addToolCall, updateToolCall,
    recordTokens, reset,
  }
})
