/**
 * AI信息聚合模块
 */

const AggregateAPI = {
    async collect(sources) {
        const response = await fetch('/api/aggregate/collect', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ sources: sources || null, force: false })
        });
        if (!response.ok) {
            const error = await response.json();
            throw new Error(error.error || '采集失败');
        }
        return await response.json();
    },
    
    async list(source, limit = 50, offset = 0) {
        const params = new URLSearchParams();
        if (source) params.append('source', source);
        params.append('limit', limit);
        params.append('offset', offset);
        
        const response = await fetch(`/api/aggregate/list?${params}`);
        if (!response.ok) throw new Error('获取列表失败');
        return await response.json();
    },
    
    async search(query, topK = 10) {
        const response = await fetch('/api/aggregate/search', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ query, top_k: topK })
        });
        if (!response.ok) throw new Error('搜索失败');
        return await response.json();
    },
    
    async generateSummary(id) {
        const response = await fetch(`/api/aggregate/summary/${id}`, {
            method: 'POST'
        });
        if (!response.ok) throw new Error('生成摘要失败');
        return await response.json();
    },
    
    async stats() {
        const response = await fetch('/api/aggregate/stats');
        if (!response.ok) throw new Error('获取统计失败');
        return await response.json();
    }
};

let aggregateItems = [];

async function collectAll() {
    const btn = document.getElementById('collect-btn');
    const status = document.getElementById('collect-status');
    
    btn.disabled = true;
    btn.textContent = '⏳ 采集中...';
    status.textContent = '正在采集GitHub、Hacker News、RSS、ArXiv...（后台运行，可刷新页面查看进度）';
    status.style.color = '#1e40af';
    
    try {
        const controller = new AbortController();
        const timeoutId = setTimeout(() => controller.abort(), 300000);
        
        const response = await fetch('/api/aggregate/collect', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ sources: null, force: false }),
            signal: controller.signal
        });
        
        clearTimeout(timeoutId);
        
        if (!response.ok) {
            const error = await response.json();
            throw new Error(error.error || '采集失败');
        }
        
        const result = await response.json();
        
        status.textContent = `采集完成！共获取 ${result.collected_count} 条内容`;
        status.style.color = '#059669';
        
        result.records.forEach(record => {
            console.log(`${record.source}: ${record.count} 条, 状态: ${record.status}`);
        });
        
        await loadAggregateStats();
        await loadAggregateList();
        
    } catch (error) {
        if (error.name === 'AbortError') {
            status.textContent = '采集正在后台运行，请稍后刷新页面查看结果';
            status.style.color = '#d97706';
            setInterval(loadAggregateStats, 10000);
        } else {
            status.textContent = `采集失败: ${error.message}`;
            status.style.color = '#dc2626';
        }
    }
    
    btn.disabled = false;
    btn.textContent = '🚀 开始采集';
}

async function loadAggregateStats() {
    try {
        const stats = await AggregateAPI.stats();
        
        document.getElementById('agg-total').textContent = stats.total_items;
        document.getElementById('agg-github').textContent = stats.by_source.github || 0;
        document.getElementById('agg-hn').textContent = stats.by_source.hackernews || 0;
        document.getElementById('agg-arxiv').textContent = (stats.by_source.arxiv || 0) + (stats.by_source.rss || 0);
        
    } catch (error) {
        console.error('加载统计失败:', error);
    }
}

async function loadAggregateList() {
    const container = document.getElementById('aggregate-list');
    const sourceFilter = document.getElementById('agg-source-filter').value;
    
    try {
        const result = await AggregateAPI.list(sourceFilter, 50, 0);
        aggregateItems = result.items;
        
        if (result.items.length === 0) {
            container.innerHTML = `
                <div style="text-align: center; color: #666; padding: 40px;">
                    <p>暂无内容，点击"开始采集"获取最新AI动态</p>
                </div>
            `;
            return;
        }
        
        container.innerHTML = result.items.map(item => renderAggregateItem(item)).join('');
        
    } catch (error) {
        container.innerHTML = `<div style="color: #dc3545; padding: 20px;">加载失败: ${error.message}</div>`;
    }
}

function renderAggregateItem(item) {
    const sourceIcon = getSourceIcon(item.source);
    const sourceColor = getSourceColor(item.source);
    const timeStr = formatAggregateTime(item.collected_at);
    
    return `
        <div class="aggregate-item" style="
            background: white;
            border: 1px solid #eee;
            border-radius: 8px;
            padding: 15px;
            margin-bottom: 10px;
        ">
            <div style="display: flex; align-items: start; gap: 10px;">
                <div style="
                    background: ${sourceColor};
                    color: white;
                    padding: 8px 12px;
                    border-radius: 6px;
                    font-size: 12px;
                    font-weight: bold;
                ">${sourceIcon}</div>
                <div style="flex: 1;">
                    <div style="font-weight: 600; color: #333; margin-bottom: 8px;">
                        ${escapeHtml(item.title)}
                    </div>
                    ${item.summary ? `
                        <div style="background: #f0f7ff; padding: 10px; border-radius: 6px; margin-bottom: 10px; font-size: 14px; color: #555; line-height: 1.5;">
                            ${escapeHtml(item.summary)}
                        </div>
                    ` : ''}
                    <div style="font-size: 12px; color: #999; margin-bottom: 8px;">
                        ${item.author ? `作者: ${escapeHtml(item.author)} · ` : ''}
                        ${timeStr}
                    </div>
                    <a href="${item.url}" target="_blank" style="
                        display: inline-block;
                        color: #667eea;
                        text-decoration: none;
                        font-size: 13px;
                    ">查看原文 →</a>
                </div>
            </div>
        </div>
    `;
}

function getSourceIcon(source) {
    const icons = {
        'github': 'GitHub',
        'hackernews': 'HN',
        'rss': 'RSS',
        'arxiv': 'ArXiv'
    };
    return icons[source] || source;
}

function getSourceColor(source) {
    const colors = {
        'github': '#333',
        'hackernews': '#ff6600',
        'rss': '#563d7c',
        'arxiv': '#b31b1b'
    };
    return colors[source] || '#667eea';
}

function formatAggregateTime(timestamp) {
    if (!timestamp) return '';
    const date = new Date(timestamp);
    const now = new Date();
    const diff = now - date;
    
    if (diff < 60000) return '刚刚';
    if (diff < 3600000) return `${Math.floor(diff / 60000)} 分钟前`;
    if (diff < 86400000) return `${Math.floor(diff / 3600000)} 小时前`;
    if (diff < 604800000) return `${Math.floor(diff / 86400000)} 天前`;
    
    return date.toLocaleDateString('zh-CN');
}

function showAggregateDetail(id) {
    const item = aggregateItems.find(i => i.id === id);
    if (!item) return;
    
    const sourceIcon = getSourceIcon(item.source);
    const sourceColor = getSourceColor(item.source);
    
    const modal = document.createElement('div');
    modal.style.cssText = `
        position: fixed;
        top: 0;
        left: 0;
        width: 100%;
        height: 100%;
        background: rgba(0,0,0,0.5);
        display: flex;
        justify-content: center;
        align-items: center;
        z-index: 1000;
    `;
    
    modal.innerHTML = `
        <div style="
            background: white;
            border-radius: 12px;
            padding: 20px;
            max-width: 600px;
            width: 90%;
            max-height: 80vh;
            overflow-y: auto;
        ">
            <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 15px;">
                <div style="
                    background: ${sourceColor};
                    color: white;
                    padding: 6px 12px;
                    border-radius: 6px;
                    font-size: 12px;
                ">${sourceIcon}</div>
                <button onclick="this.closest('div').parentElement.remove()" style="
                    background: none;
                    border: none;
                    font-size: 20px;
                    cursor: pointer;
                    color: #999;
                ">✕</button>
            </div>
            
            <h3 style="margin-bottom: 10px; color: #333;">${escapeHtml(item.title)}</h3>
            
            ${item.author ? `<div style="color: #666; margin-bottom: 10px;">作者: ${escapeHtml(item.author)}</div>` : ''}
            
            <div style="color: #555; line-height: 1.6; margin-bottom: 15px;">
                ${escapeHtml(item.content)}
            </div>
            
            ${item.summary ? `
                <div style="background: #f0f7ff; padding: 15px; border-radius: 8px; margin-bottom: 15px;">
                    <div style="font-weight: 600; color: #667eea; margin-bottom: 8px;">📝 AI摘要</div>
                    <div style="color: #555;">${escapeHtml(item.summary)}</div>
                </div>
            ` : `
                <button onclick="generateItemSummary('${item.id}')" style="
                    background: #667eea;
                    color: white;
                    border: none;
                    padding: 8px 16px;
                    border-radius: 6px;
                    cursor: pointer;
                    margin-bottom: 15px;
                ">生成AI摘要</button>
            `}
            
            ${item.keywords.length > 0 ? `
                <div style="margin-bottom: 15px;">
                    <span style="color: #666;">关键词：</span>
                    ${item.keywords.map(k => `<span style="background: #e8f4f8; padding: 3px 8px; border-radius: 4px; margin: 2px; font-size: 12px;">${escapeHtml(k)}</span>`).join('')}
                </div>
            ` : ''}
            
            <a href="${item.url}" target="_blank" style="
                display: inline-block;
                color: #667eea;
                text-decoration: none;
                padding: 8px 16px;
                border: 1px solid #667eea;
                border-radius: 6px;
            ">查看原文 →</a>
        </div>
    `;
    
    document.body.appendChild(modal);
    modal.onclick = (e) => {
        if (e.target === modal) modal.remove();
    };
}

async function generateItemSummary(id) {
    try {
        const result = await AggregateAPI.generateSummary(id);
        
        const item = aggregateItems.find(i => i.id === id);
        if (item) {
            item.summary = result.summary;
        }
        
        showAggregateDetail(id);
        
    } catch (error) {
        alert('生成摘要失败: ' + error.message);
    }
}

async function aggregateSearch() {
    const query = document.getElementById('agg-search-input').value.trim();
    if (!query) {
        loadAggregateList();
        return;
    }
    
    const container = document.getElementById('aggregate-list');
    
    try {
        const result = await AggregateAPI.search(query, 20);
        
        if (result.results.length === 0) {
            container.innerHTML = `
                <div style="text-align: center; color: #666; padding: 40px;">
                    <p>未找到相关内容</p>
                </div>
            `;
            return;
        }
        
        aggregateItems = result.results.map(r => ({
            id: r.id,
            source: r.source,
            title: r.title,
            content: r.content,
            url: r.url,
            author: null,
            collected_at: null,
            keywords: [],
            summary: r.summary
        }));
        
        container.innerHTML = `
            <div style="margin-bottom: 15px; color: #667eea;">
                找到 ${result.results.length} 条相关内容
            </div>
            ${aggregateItems.map(item => renderAggregateItem(item)).join('')}
        `;
        
    } catch (error) {
        container.innerHTML = `<div style="color: #dc3545; padding: 20px;">搜索失败: ${error.message}</div>`;
    }
}

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

document.addEventListener('DOMContentLoaded', () => {
    loadAggregateStats();
});