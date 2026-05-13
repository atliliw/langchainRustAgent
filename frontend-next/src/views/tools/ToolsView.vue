<template>
  <div>
    <h3 style="margin:0 0 16px 0;">🔧 Tools 管理 <el-tag size="small" type="success" style="margin-left:6px;">NEW</el-tag></h3>
    <el-tabs v-model="activeTab">
      <el-tab-pane label="📦 内置工具" name="builtin">
        <el-table :data="builtinTools" stripe style="width:100%;">
          <el-table-column prop="name" label="工具名" width="140" />
          <el-table-column prop="description" label="描述" />
          <el-table-column label="操作" width="100">
            <template #default="{ row }">
              <el-button size="small" @click="testTool(row.name)">测试</el-button>
            </template>
          </el-table-column>
        </el-table>
      </el-tab-pane>
      <el-tab-pane label="🔗 MCP 服务器" name="mcp">
        <div style="margin-bottom:12px;display:flex;gap:8px;">
          <el-input v-model="mcpUrl" placeholder="MCP Server URL" style="width:300px;" />
          <el-input v-model="mcpName" placeholder="服务器名称" style="width:150px;" />
          <el-button type="primary" @click="connectMcp" :loading="mcpLoading">连接测试</el-button>
        </div>
        <el-table :data="mcpTools" stripe v-if="mcpTools.length > 0">
          <el-table-column prop="name" label="工具名" width="140" />
          <el-table-column prop="description" label="描述" />
        </el-table>
        <el-empty v-else-if="!mcpLoading" description="尚未连接 MCP 服务器" />
      </el-tab-pane>
    </el-tabs>
  </div>
</template>
<script setup lang="ts">
import { ref, onMounted } from 'vue'
import http from '../../api/client'
import { ElMessage } from 'element-plus'
const activeTab = ref('builtin')
const builtinTools = ref<any[]>([])
const mcpUrl = ref('')
const mcpName = ref('')
const mcpTools = ref<any[]>([])
const mcpLoading = ref(false)
onMounted(async () => {
  try { const { data } = await http.get('/v2/tools'); builtinTools.value = data.tools || [] } catch { /* ignore */ }
})
async function testTool(name: string) { ElMessage.info(`测试工具: ${name}`) }
async function connectMcp() {
  if (!mcpUrl.value) { ElMessage.warning('请输入 MCP URL'); return }
  mcpLoading.value = true
  try {
    const { data } = await http.post('/v2/mcp/tools', { url: mcpUrl.value })
    mcpTools.value = data.tools || []
    ElMessage.success(`已发现 ${mcpTools.value.length} 个工具`)
  } catch (e: any) { ElMessage.error(e.message) } finally { mcpLoading.value = false }
}
</script>