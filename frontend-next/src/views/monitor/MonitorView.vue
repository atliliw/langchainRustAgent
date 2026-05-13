<template>
  <div>
    <h3 style="margin:0 0 16px 0;">📊 监控面板 <el-tag size="small" type="success" style="margin-left:6px;">NEW</el-tag></h3>
    <div style="display:grid;grid-template-columns:repeat(4,1fr);gap:12px;margin-bottom:16px;">
      <el-card v-for="s in stats" :key="s.label">
        <div style="font-size:24px;font-weight:bold;color:#7c3aed;">{{ s.value }}</div>
        <div style="font-size:13px;color:#64748b;">{{ s.label }}</div>
      </el-card>
    </div>
    <el-card>
      <template #header><span>📈 Token 消耗趋势</span></template>
      <div ref="chartRef" style="height:300px;"></div>
    </el-card>
  </div>
</template>
<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import http from '../../api/client'
const chartRef = ref<HTMLElement>()
const stats = ref([
  { label: '总 Token', value: '0' }, { label: '总费用', value: '$0.00' },
  { label: '本月 Token', value: '0' }, { label: 'API 调用', value: '0' },
])
let chart: any = null
onMounted(async () => {
  try {
    const { data } = await http.get('/v2/stats')
    stats.value = [
      { label: '总 Token', value: (data.total_tokens || 0).toLocaleString() },
      { label: '总费用', value: `$${(data.total_cost || 0).toFixed(4)}` },
      { label: '本月 Token', value: (data.today_tokens || 0).toLocaleString() },
      { label: 'API 调用', value: ((data.by_endpoint || []).reduce((a:any,b:any)=>a+b.count,0) || 0).toString() },
    ]
  } catch { /* ignore */ }
  if (chartRef.value) {
    try {
      const echarts = (await import('echarts')).default
      chart = echarts.init(chartRef.value)
      chart.setOption({
        tooltip: { trigger: 'axis' },
        xAxis: { type: 'category', data: ['周一','周二','周三','周四','周五','周六','周日'] },
        yAxis: { type: 'value' },
        series: [{ name: 'Token', type: 'line', smooth: true, data: [120,200,150,80,70,110,130], areaStyle: {} }],
      })
    } catch { /* echarts not installed */ }
  }
})
onUnmounted(() => { chart?.dispose() })
</script>