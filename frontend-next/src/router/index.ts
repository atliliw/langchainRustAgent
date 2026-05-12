import { createRouter, createWebHistory } from 'vue-router'

const router = createRouter({
  history: createWebHistory('/console/'),
  routes: [
    {
      path: '/',
      redirect: '/playground',
    },
    {
      path: '/playground',
      name: 'playground',
      component: () => import('../views/playground/PlaygroundView.vue'),
      meta: { title: 'Agent Playground', icon: '🛝' },
    },
    {
      path: '/tools',
      name: 'tools',
      component: () => import('../views/tools/ToolsView.vue'),
      meta: { title: 'Tools 管理', icon: '🔧' },
    },
    {
      path: '/monitor',
      name: 'monitor',
      component: () => import('../views/monitor/MonitorView.vue'),
      meta: { title: '监控面板', icon: '📊' },
    },
    {
      path: '/evaluate',
      name: 'evaluate',
      component: () => import('../views/evaluate/EvaluateView.vue'),
      meta: { title: '评估中心', icon: '🧪' },
    },
    {
      path: '/settings',
      name: 'settings',
      component: () => import('../views/settings/SettingsView.vue'),
      meta: { title: '系统设置', icon: '⚙️' },
    },
  ],
})

export default router
