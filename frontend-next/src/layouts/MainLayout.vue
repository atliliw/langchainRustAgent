<template>
  <el-container style="height: 100vh;">
    <el-aside width="220px" style="background:#1e293b;overflow:hidden;">
      <div style="padding:20px 16px;color:#fff;font-weight:bold;font-size:18px;border-bottom:1px solid #334155;">
        🤖 Agent 平台
      </div>
      <el-menu
        :default-active="route.path"
        background-color="#1e293b"
        text-color="#cbd5e1"
        active-text-color="#fff"
        style="border:none;"
        @select="onMenuSelect"
      >
        <el-menu-item index="/playground">
          <span style="margin-right:8px;">🛝</span>
          <span>Playground</span>
          <el-tag size="small" type="success" style="margin-left:auto;height:18px;line-height:16px;font-size:10px;border:0;">NEW</el-tag>
        </el-menu-item>
        <el-menu-item index="/tools">
          <span style="margin-right:8px;">🔧</span>
          <span>Tools 管理</span>
          <el-tag size="small" type="success" style="margin-left:auto;height:18px;line-height:16px;font-size:10px;border:0;">NEW</el-tag>
        </el-menu-item>
        <el-menu-item index="/monitor">
          <span style="margin-right:8px;">📊</span>
          <span>监控面板</span>
          <el-tag size="small" type="success" style="margin-left:auto;height:18px;line-height:16px;font-size:10px;border:0;">NEW</el-tag>
        </el-menu-item>
        <el-menu-item index="/evaluate">
          <span style="margin-right:8px;">🧪</span>
          <span>评估中心</span>
          <el-tag size="small" type="success" style="margin-left:auto;height:18px;line-height:16px;font-size:10px;border:0;">NEW</el-tag>
        </el-menu-item>
        <el-menu-item index="/settings">
          <span style="margin-right:8px;">⚙️</span>
          <span>系统设置</span>
          <el-tag size="small" type="success" style="margin-left:auto;height:18px;line-height:16px;font-size:10px;border:0;">NEW</el-tag>
        </el-menu-item>
        <div style="border-top:1px solid #334155;margin:8px 16px;padding-top:8px;">
          <div style="font-size:11px;color:#64748b;padding:4px 8px;">📦 旧版功能</div>
          <a href="/" style="display:block;padding:8px 12px;color:#94a3b8;font-size:13px;text-decoration:none;border-radius:6px;transition:background 0.2s;"
             onmouseover="this.style.background='#334155'" onmouseout="this.style.background='transparent'"
          >→ 前往旧版控制台</a>
        </div>
      </el-menu>
    </el-aside>
    <el-container>
      <el-header style="height:50px;background:#fff;border-bottom:1px solid #e2e8f0;display:flex;align-items:center;padding:0 20px;justify-content:space-between;">
        <span style="font-weight:600;color:#1e293b;">{{ route.meta.title || 'Agent 平台' }}</span>
        <span style="font-size:12px;color:#94a3b8;">v1.0</span>
      </el-header>
      <el-main style="background:#f8fafc;padding:20px;">
        <router-view />
      </el-main>
    </el-container>
  </el-container>
</template>

<script setup lang="ts">
import { useRoute, useRouter } from 'vue-router'
const route = useRoute()
const router = useRouter()
function onMenuSelect(index: string) { router.push(index) }
</script>
