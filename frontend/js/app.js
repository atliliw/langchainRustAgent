const API_BASE = '/api';
let currentSessionId = null;
let displayedText = '';
let searchHistory = JSON.parse(localStorage.getItem('searchHistory') || '[]');
let currentPage = 1;
let pageSize = 10;

async function fetchStats() {
    try {
        const res = await fetch(`${API_BASE}/stats`);
        const data = await res.json();
        document.getElementById('total-docs').textContent = data.total_documents;
        document.getElementById('vector-size').textContent = data.vector_size;
        document.getElementById('bm25-chunks').textContent = data.bm25_chunks;
        document.getElementById('sessions-count').textContent = data.conversation_sessions || 0;
    } catch (e) {
        // 静默处理
    }
}

function showTab(name) {
    document.querySelectorAll('.nav-item').forEach(t => t.classList.remove('active'));
    document.querySelectorAll('.card').forEach(c => { if (c.id.endsWith('-tab')) c.classList.add('hidden'); });
    document.querySelector(`.nav-item[onclick="showTab('${name}')"]`).classList.add('active');
    document.getElementById(`${name}-tab`).classList.remove('hidden');
    if (name === 'chat') loadSessions();
    if (name === 'sessions') loadAllSessions();
    if (name === 'langgraph') loadLangGraphInfo();
    if (name === 'documents') loadDocuments();
    if (name === 'monitor') loadMonitorStats();
    if (name === 'compress') { updateCompressExp(); showModeDetail('layered'); }
}

async function loadDocuments() {
    const listEl = document.getElementById('documents-list');
    listEl.innerHTML = '<div class="loading">加载文档列表...</div>';
    try {
        const res = await fetch(`${API_BASE}/documents`);
        const documents = await res.json();
        if (documents.length === 0) { listEl.innerHTML = '<p style="color: #666;">暂无文档，请先上传</p>'; return; }
        let html = '<div style="margin-bottom:15px;display:flex;gap:10px;">';
        html += `<button class="btn btn-small btn-danger" onclick="batchDeleteDocuments()">🗑️ 批量删除</button>`;
        html += `<button class="btn btn-small" onclick="importSession()">📥 导入会话</button>`;
        html += '</div>';
        html += '<table style="width: 100%; border-collapse: collapse;"><thead><tr style="background: #f1f5f9;">';
        html += '<th style="padding: 12px; border: 1px solid #e2e8f0;width:40px;"><input type="checkbox" onchange="toggleAllDocs(this)"></th>';
        html += '<th style="padding: 12px; border: 1px solid #e2e8f0;">文档标题</th><th style="padding: 12px; border: 1px solid #e2e8f0;">Chunk数量</th><th style="padding: 12px; border: 1px solid #e2e8f0;">内容预览</th><th style="padding: 12px; border: 1px solid #e2e8f0;">操作</th></tr></thead><tbody>';
        documents.forEach(doc => {
            html += `<tr>
                <td style="padding: 12px; border: 1px solid #e2e8f0;text-align:center;"><input type="checkbox" class="doc-checkbox" value="${doc.id}"></td>
                <td style="padding: 12px; border: 1px solid #e2e8f0;">${escapeHtml(doc.title)}</td>
                <td style="padding: 12px; border: 1px solid #e2e8f0; text-align: center;">${doc.chunk_count}</td>
                <td style="padding: 12px; border: 1px solid #e2e8f0; color: #64748b; font-size: 13px;">${escapeHtml(doc.content_preview)}...</td>
                <td style="padding: 12px; border: 1px solid #e2e8f0;">
                    <button class="btn btn-small" onclick="previewDocument('${doc.id}', '${escapeHtml(doc.title)}')">预览</button>
                    <button class="btn btn-small" onclick="addDocumentTags('${doc.id}')">🏷️</button>
                    <button class="btn btn-small btn-danger" onclick="deleteDocument('${doc.id}', '${escapeHtml(doc.title)}')">删除</button>
                </td>
            </tr>`;
        });
        html += '</tbody></table>';
        listEl.innerHTML = html;
    } catch (e) { listEl.innerHTML = `<div class="error">加载失败: ${e.message}</div>`; }
}

function toggleAllDocs(cb) {
    document.querySelectorAll('.doc-checkbox').forEach(c => c.checked = cb.checked);
}

async function previewDocument(parentId, title) {
    try {
        const res = await fetch(`${API_BASE}/search/bm25`, {
            method: 'POST',
            headers: {'Content-Type': 'application/json'},
            body: JSON.stringify({query: title, top_k: 20})
        });
        const data = await res.json();
        const chunks = data.results.filter(r => r.metadata?.parent_id === parentId);
        
        const modal = document.getElementById('detail-modal');
        const contentDiv = document.getElementById('detail-content');
        
        let html = `<h3 style="color:#1e40af;margin-bottom:16px;">📄 ${escapeHtml(title)} (${chunks.length} chunks)</h3>`;
        html += '<div style="display:grid;gap:12px;max-height:70vh;overflow-y:auto;">';
        chunks.forEach((chunk, idx) => {
            html += `<div class="chunk-card">
                <div class="chunk-header">
                    <span style="background:#1e40af;color:white;padding:4px 10px;border-radius:4px;font-size:12px;">Chunk ${idx + 1}</span>
                    <span style="font-size:12px;color:#64748b;">BM25: ${chunk.score.toFixed(2)}</span>
                </div>
                <div class="chunk-content">${escapeHtml(chunk.content)}</div>
            </div>`;
        });
        html += '</div>';
        
        contentDiv.innerHTML = html;
        modal.style.display = 'block';
    } catch (e) { alert('加载预览失败: ' + e.message); }
}

async function deleteDocument(parentId, filename) {
    if (!confirm(`确定删除文档 "${filename}"？\n将同时删除 BM25 chunks 和向量数据。`)) return;
    try {
        const res = await fetch(`${API_BASE}/documents/${parentId}`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({filename}) });
        const data = await res.json();
        if (data.success) { alert(data.message); loadDocuments(); fetchStats(); }
        else { alert('删除失败: ' + (data.error || '未知错误')); }
    } catch (e) { alert('删除失败: ' + e.message); }
}

async function uploadFile(file) {
    const allowed = ['.txt', '.pdf', '.md', '.json', '.csv'];
    const ext = file.name.substring(file.name.lastIndexOf('.')).toLowerCase();
    if (!allowed.includes(ext)) { showResult('upload-result', 'error', `不支持的文件类型: ${ext}`); return; }
    showResult('upload-result', 'loading', '正在上传和处理...');
    document.getElementById('upload-progress').classList.remove('hidden');
    const formData = new FormData();
    formData.append('file', file);
    try {
        const res = await fetch(`${API_BASE}/upload`, { method: 'POST', body: formData });
        const data = await res.json();
        if (data.success) showResult('upload-result', 'success', `${data.message}<br>文档块数: ${data.chunk_count}`);
        else showResult('upload-result', 'error', data.error || '上传失败');
        fetchStats();
    } catch (e) { showResult('upload-result', 'error', `上传失败: ${e.message}`); }
    document.getElementById('upload-progress').classList.add('hidden');
}

async function uploadMultipleFiles(files) {
    const allowed = ['.txt', '.pdf', '.md', '.json', '.csv'];
    let successCount = 0;
    let failCount = 0;
    let totalChunks = 0;
    
    showResult('upload-result', 'loading', `正在批量上传 ${files.length} 个文件...`);
    document.getElementById('upload-progress').classList.remove('hidden');
    
    for (let i = 0; i < files.length; i++) {
        const file = files[i];
        const ext = file.name.substring(file.name.lastIndexOf('.')).toLowerCase();
        if (!allowed.includes(ext)) { failCount++; continue; }
        
        const formData = new FormData();
        formData.append('file', file);
        
        try {
            const res = await fetch(`${API_BASE}/upload`, { method: 'POST', body: formData });
            const data = await res.json();
            if (data.success) { successCount++; totalChunks += data.chunk_count; }
            else failCount++;
            
            const progress = ((i + 1) / files.length) * 100;
            document.getElementById('progress-bar').style.width = progress + '%';
            showResult('upload-result', 'loading', `上传进度: ${i+1}/${files.length}`);
        } catch (e) { failCount++; }
    }
    
    document.getElementById('upload-progress').classList.add('hidden');
    showResult('upload-result', 'success', `批量上传完成<br>成功: ${successCount} 个<br>失败: ${failCount} 个<br>总块数: ${totalChunks}`);
    fetchStats(); loadDocuments();
}

async function bm25Search() {
    const query = document.getElementById('bm25-query').value.trim();
    const topK = parseInt(document.getElementById('bm25-top-k').value) || 5;
    if (!query) { showResult('bm25-results', 'error', '请输入搜索内容'); return; }
    saveSearchHistory(query);
    showResult('bm25-results', 'loading', '正在搜索...');
    currentPage = 1;
    try {
        const res = await fetch(`${API_BASE}/search/bm25`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({query, top_k: topK}) });
        const data = await res.json();
        window._bm25Results = data.results;
        document.getElementById('bm25-results').innerHTML = `<h3 style="color: #4caf50; margin-top: 15px;">BM25检索 (${data.total_count}条)</h3>${renderSearchHistory()}${renderPaginatedResults('bm25', data.results)}`;
    } catch (e) { showResult('bm25-results', 'error', `搜索失败: ${e.message}`); }
}

async function vectorSearch() {
    const query = document.getElementById('vector-query').value.trim();
    const topK = parseInt(document.getElementById('vector-top-k').value) || 5;
    if (!query) { showResult('vector-results', 'error', '请输入搜索内容'); return; }
    saveSearchHistory(query);
    showResult('vector-results', 'loading', '正在搜索...');
    currentPage = 1;
    try {
        const res = await fetch(`${API_BASE}/search/vector`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({query, top_k: topK}) });
        const data = await res.json();
        window._vectorResults = data.results;
        document.getElementById('vector-results').innerHTML = `<h3 style="color: #e94560; margin-top: 15px;">向量检索 (${data.total_count}条)</h3>${renderSearchHistory()}${renderPaginatedResults('vector', data.results)}`;
    } catch (e) { showResult('vector-results', 'error', `搜索失败: ${e.message}`); }
}

async function compareSearch() {
    const query = document.getElementById('compare-query').value.trim();
    const topK = parseInt(document.getElementById('compare-top-k').value) || 5;
    if (!query) { showResult('compare-results', 'error', '请输入搜索内容'); return; }
    showResult('compare-results', 'loading', '正在对比三种搜索模式...');
    try {
        const [vectorRes, bm25Res, hybridRes] = await Promise.all([
            fetch(`${API_BASE}/search/vector`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({query, top_k: topK}) }),
            fetch(`${API_BASE}/search/bm25`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({query, top_k: topK}) }),
            fetch(`${API_BASE}/search/hybrid`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({query, top_k: topK}) })
        ]);
        const vectorData = await vectorRes.json();
        const bm25Data = await bm25Res.json();
        const hybridData = await hybridRes.json();
        document.getElementById('compare-results').innerHTML = `<div style="display: grid; grid-template-columns: repeat(3, 1fr); gap: 15px; margin-top: 20px;"><div><h3 style="color: #e94560; margin-bottom: 10px;">向量检索 (${vectorData.total_count}条)</h3>${renderResults(vectorData.results)}</div><div><h3 style="color: #4caf50; margin-bottom: 10px;">BM25检索 (${bm25Data.total_count}条)</h3>${renderResults(bm25Data.results)}</div><div><h3 style="color: #c73e54; margin-bottom: 10px;">混合检索 (${hybridData.total_count}条)</h3>${renderResults(hybridData.results)}</div></div>`;
    } catch (e) { showResult('compare-results', 'error', `搜索失败: ${e.message}`); }
}

function renderResults(results) {
    if (results.length === 0) return '<p style="color: #64748b;">无结果</p>';
    window._searchResults = results;
    return results.map((r, idx) => {
        const source = r.source || 'unknown';
        const isBM25 = source === 'bm25';
        const scoreDisplay = isBM25 ? r.score.toFixed(2) : (r.score * 100).toFixed(1) + '%';
        const scoreLabel = isBM25 ? 'BM25分数' : '相似度';
        const fullId = r.id || 'unknown';
        const parentId = r.metadata?.parent_id || '';
        const isMerged = r.metadata?.is_merged || false;
        const fullContent = r.content || '';
        const shortContent = fullContent.length > 150 ? fullContent.substring(0, 150) + '...' : fullContent;
        
        return `<div class="result-item" style="padding:20px;margin-bottom:16px;background:white;border-radius:10px;border:1px solid #cbd5e1;box-shadow:0 1px 3px rgba(0,0,0,0.1);">
            <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:12px;">
                <div style="display:flex;align-items:center;gap:12px;">
                    <span style="background:#1e40af;color:white;padding:4px 10px;border-radius:6px;font-size:13px;font-weight:600;">#${idx + 1}</span>
                    <span style="background:${isBM25 ? '#059669' : '#3b82f6'};color:white;padding:4px 10px;border-radius:6px;font-size:13px;font-weight:500;">${source}</span>
                    ${isMerged ? '<span style="background:#f59e0b;color:white;padding:4px 10px;border-radius:6px;font-size:13px;">merged</span>' : ''}
                </div>
                <div style="display:flex;align-items:center;gap:8px;">
                    <span style="font-size:13px;color:#475569;">${scoreLabel}:</span>
                    <span style="font-size:20px;font-weight:700;color:#059669;">${scoreDisplay}</span>
                </div>
            </div>
            
            <div style="background:#f1f5f9;padding:12px;border-radius:8px;margin-bottom:12px;display:flex;align-items:center;gap:8px;">
                <span style="font-size:13px;color:#1e40af;font-weight:600;">ID:</span>
                <span style="font-family:monospace;font-size:12px;color:#1e293b;word-break:break-all;flex:1;">${fullId}</span>
                <button onclick="copyToClipboard('${fullId}')" style="padding:4px 10px;background:#3b82f6;color:white;border:none;border-radius:4px;font-size:12px;cursor:pointer;">复制</button>
            </div>
            
            <div style="color:#1e293b;line-height:1.8;font-size:15px;white-space:pre-wrap;word-break:break-word;margin-bottom:16px;padding:12px;background:#f8fafc;border-radius:8px;">${escapeHtml(shortContent)}</div>
            
            <div style="display:flex;gap:10px;">
                <button onclick="openDetailModal(${idx})" style="padding:10px 20px;background:#1e40af;color:white;border:none;border-radius:8px;font-size:14px;cursor:pointer;font-weight:500;">📄 查看完整详情</button>
            </div>
        </div>`;
    }).join('');
}

function copyToClipboard(text) {
    navigator.clipboard.writeText(text).then(() => {
        showToast('已复制到剪贴板');
    }).catch(err => {
        console.error('复制失败:', err);
    });
}

function saveSearchHistory(query) {
    if (!query || query.length < 2) return;
    searchHistory = searchHistory.filter(h => h !== query);
    searchHistory.unshift(query);
    searchHistory = searchHistory.slice(0, 20);
    localStorage.setItem('searchHistory', JSON.stringify(searchHistory));
}

function renderSearchHistory() {
    if (searchHistory.length === 0) return '';
    let html = '<div class="search-history"><span style="color:#64748b;font-size:13px;">搜索历史：</span>';
    searchHistory.slice(0, 5).forEach(h => {
        html += `<span class="history-item" onclick="quickSearch('${escapeHtml(h)}')">${escapeHtml(h.length > 20 ? h.substring(0,20)+'...' : h)}</span>`;
    });
    html += '</div>';
    return html;
}

function quickSearch(query) {
    const activeTab = document.querySelector('.nav-item.active');
    if (activeTab) {
        const tabName = activeTab.getAttribute('onclick').match(/showTab\('(\w+)'\)/)?.[1];
        if (tabName === 'bm25') document.getElementById('bm25-query').value = query;
        else if (tabName === 'vector') document.getElementById('vector-query').value = query;
        else if (tabName === 'compare') document.getElementById('compare-query').value = query;
    }
}

function renderPaginatedResults(type, results) {
    if (results.length === 0) return '<p style="color: #64748b;">无结果</p>';
    window._searchResults = results;
    
    const totalPages = Math.ceil(results.length / pageSize);
    const start = (currentPage - 1) * pageSize;
    const end = start + pageSize;
    const pageResults = results.slice(start, end);
    
    let html = renderResults(pageResults);
    
    if (totalPages > 1) {
        html += `<div class="pagination">
            <button class="page-btn" onclick="changePage('${type}', -1)" ${currentPage === 1 ? 'disabled' : ''}>上一页</button>
            <span class="page-info">${currentPage}/${totalPages}</span>
            <button class="page-btn" onclick="changePage('${type}', 1)" ${currentPage === totalPages ? 'disabled' : ''}>下一页</button>
        </div>`;
    }
    
    return html;
}

function changePage(type, delta) {
    currentPage += delta;
    const results = type === 'bm25' ? window._bm25Results : 
                    type === 'vector' ? window._vectorResults : window._searchResults;
    const containerId = type === 'bm25' ? 'bm25-results' : 
                        type === 'vector' ? 'vector-results' : 'compare-results';
    
    const totalPages = Math.ceil(results.length / pageSize);
    currentPage = Math.max(1, Math.min(currentPage, totalPages));
    
    document.getElementById(containerId).innerHTML = 
        `<h3 style="color: #4caf50; margin-top: 15px;">${type}检索 (${results.length}条)</h3>` +
        renderSearchHistory() + 
        renderPaginatedResults(type, results);
}

function openDetailModal(idx) {
    const r = window._searchResults[idx];
    if (!r) return;
    
    const source = r.source || 'unknown';
    const isBM25 = source === 'bm25';
    const scoreDisplay = isBM25 ? r.score.toFixed(4) : (r.score * 100).toFixed(2) + '%';
    const fullId = r.id || 'unknown';
    const parentId = r.metadata?.parent_id || '';
    const isMerged = r.metadata?.is_merged || false;
    const fullContent = r.content || '';
    const metadata = r.metadata || {};
    
    const modal = document.getElementById('detail-modal');
    const contentDiv = document.getElementById('detail-content');
    
    contentDiv.innerHTML = `
        <div style="display:grid;gap:16px;">
            <div style="display:grid;grid-template-columns:repeat(2,1fr);gap:12px;">
                <div style="background:#f1f5f9;padding:20px;border-radius:10px;border:1px solid #cbd5e1;">
                    <div style="font-size:13px;color:#475569;margin-bottom:6px;">搜索来源</div>
                    <div style="font-size:18px;font-weight:700;color:#1e40af;">${source}</div>
                </div>
                <div style="background:#f1f5f9;padding:20px;border-radius:10px;border:1px solid #cbd5e1;">
                    <div style="font-size:13px;color:#475569;margin-bottom:6px;">${isBM25 ? 'BM25分数' : '相似度'}</div>
                    <div style="font-size:18px;font-weight:700;color:#059669;">${scoreDisplay}</div>
                </div>
            </div>
            
            <div style="background:#f1f5f9;padding:16px;border-radius:10px;border:1px solid #cbd5e1;">
                <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:8px;">
                    <span style="font-size:13px;color:#1e40af;font-weight:600;">Chunk ID</span>
                    <button onclick="copyToClipboard('${fullId}')" style="padding:6px 12px;background:#3b82f6;color:white;border:none;border-radius:6px;font-size:13px;cursor:pointer;">复制</button>
                </div>
                <div style="font-family:monospace;font-size:14px;color:#1e293b;word-break:break-all;background:white;padding:10px;border-radius:6px;line-height:1.5;">${fullId}</div>
            </div>
            
            ${parentId ? `<div style="background:#f1f5f9;padding:16px;border-radius:10px;border:1px solid #cbd5e1;">
                <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:8px;">
                    <span style="font-size:13px;color:#1e40af;font-weight:600;">Parent ID (文档ID)</span>
                    <button onclick="copyToClipboard('${parentId}')" style="padding:6px 12px;background:#3b82f6;color:white;border:none;border-radius:6px;font-size:13px;cursor:pointer;">复制</button>
                </div>
                <div style="font-family:monospace;font-size:14px;color:#1e293b;word-break:break-all;background:white;padding:10px;border-radius:6px;line-height:1.5;">${parentId}</div>
            </div>` : ''}
            
            ${isMerged ? `<div style="background:#fef3c7;padding:16px;border-radius:10px;border:1px solid #fcd34d;">
                <div style="font-size:15px;color:#b45309;font-weight:600;">⚠️ 此结果由多个chunk合并而成</div>
            </div>` : ''}
            
            <div style="background:#f1f5f9;padding:16px;border-radius:10px;border:1px solid #cbd5e1;">
                <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:10px;">
                    <span style="font-size:13px;color:#1e40af;font-weight:600;">完整内容 (${fullContent.length} 字符)</span>
                    <button onclick="copyText()" style="padding:6px 12px;background:#059669;color:white;border:none;border-radius:6px;font-size:13px;cursor:pointer;">复制内容</button>
                </div>
                <div id="detail-full-content" style="font-size:15px;color:#1e293b;line-height:1.8;white-space:pre-wrap;word-break:break-word;background:white;padding:14px;border-radius:8px;max-height:500px;overflow-y:auto;">${escapeHtml(fullContent)}</div>
            </div>
            
            ${Object.keys(metadata).length > 0 ? `<div style="background:#f1f5f9;padding:16px;border-radius:10px;border:1px solid #cbd5e1;">
                <div style="font-size:13px;color:#1e40af;font-weight:600;margin-bottom:10px;">Metadata</div>
                <pre style="font-size:13px;color:#1e293b;background:white;padding:14px;border-radius:8px;overflow-x:auto;line-height:1.5;">${escapeHtml(JSON.stringify(metadata, null, 2))}</pre>
            </div>` : ''}
        </div>
    `;
    
    window._currentContent = fullContent;
    modal.style.display = 'block';
}

function copyText() {
    copyToClipboard(window._currentContent);
}

function closeDetailModal() {
    document.getElementById('detail-modal').style.display = 'none';
}

document.addEventListener('keydown', function(e) {
    if (e.key === 'Escape') closeDetailModal();
});

async function clearAll() {
    if (!confirm('确定要清空所有数据吗？包括文档、索引和对话历史？')) return;
    try {
        const res = await fetch(`${API_BASE}/clear`, {method: 'POST'});
        const data = await res.json();
        if (data.success) { showResult('upload-result', 'success', data.message); currentSessionId = null; document.getElementById('chat-messages').innerHTML = '<div style="text-align: center; color: #a2a2a2; padding: 40px;">数据已清空，开始新对话</div>'; fetchStats(); loadSessions(); }
        else showResult('upload-result', 'error', data.error);
    } catch (e) { showResult('upload-result', 'error', `清空失败: ${e.message}`); }
}

async function loadSessions() {
    try {
        const res = await fetch(`${API_BASE}/chat/sessions`);
        const sessions = await res.json();
        let html = '<div style="margin-bottom:10px;">';
        html += '<input type="text" id="session-search" placeholder="搜索会话..." style="width:100%;padding:8px;border:1px solid #e2e8f0;border-radius:6px;" onkeypress="if(event.key===\'Enter\')searchSessions(this.value)">';
        html += `<button class="btn btn-small" onclick="importSession()" style="margin-top:8px;">📥 导入</button>`;
        html += '</div>';
        sessions.forEach(s => {
            const isActive = s.session_id === currentSessionId;
            const title = s.title || s.preview || '新对话';
            const shortId = s.session_id.substring(0, 8) + '...';
            html += `<div class="session-item ${isActive ? 'active' : ''}" onclick="loadSession('${s.session_id}')">
                <div class="session-title">${escapeHtml(title)}</div>
                <div class="time">${formatTime(s.created_at)} (${s.message_count}条)</div>
                <div style="font-size:10px;color:#94a3b8;font-family:monospace;margin-top:2px;">ID: ${shortId}</div>
            </div>`;
        });
        document.getElementById('session-list').innerHTML = html || '<div style="color: #a2a2a2; font-size: 12px;">暂无历史会话</div>';
    } catch (e) { document.getElementById('session-list').innerHTML = `<div class="error">加载失败: ${e.message}</div>`; }
}

async function loadAllSessions() {
    try {
        const res = await fetch(`${API_BASE}/chat/sessions`);
        const sessions = await res.json();
        let html = '<div style="display: grid; gap: 10px;">';
        sessions.forEach(s => { html += `<div class="session-item" onclick="loadSession('${s.session_id}'); showTab('chat');"><div style="display: flex; justify-content: space-between;"><span>${s.preview || '新对话'}</span><button class="btn btn-small btn-danger" onclick="event.stopPropagation(); deleteSession('${s.session_id}')">删除</button></div><div class="time">${formatTime(s.created_at)} | ${s.message_count}条消息</div></div>`; });
        html += '</div>';
        document.getElementById('sessions-list').innerHTML = html || '<div style="color: #a2a2a2;">暂无会话</div>';
    } catch (e) { document.getElementById('sessions-list').innerHTML = `<div class="error">加载失败: ${e.message}</div>`; }
}

async function loadSession(sessionId) {
    currentSessionId = sessionId;
    updateSessionDisplay(sessionId);
    try {
        const res = await fetch(`${API_BASE}/chat/history/${sessionId}`);
        const messages = await res.json();
        // 计算本次对话的总 token
        var totalTokens = 0;
        messages.forEach(function(m) { if (m.tokens) totalTokens += m.tokens; });
        let html = '<div style="margin-bottom:10px;display:flex;gap:10px;align-items:center;">';
        html += `<button class="btn btn-small" onclick="exportSession('${sessionId}')">📤 导出会话</button>`;
        html += `<span style="margin-left:auto;font-size:12px;color:#64748b;">总消耗: <strong style="color:#1e40af;">${totalTokens}</strong> tokens</span>`;
        html += '</div>';
        messages.forEach(m => {
            const roleClass = m.role === 'user' ? 'user' : 'assistant';
            const msgId = m.id;
            const tokenBadge = m.tokens ? `<span style="background:#e2e8f0;color:#64748b;border-radius:4px;padding:1px 6px;font-size:10px;margin-left:8px;">${m.tokens}t</span>` : '';
            html += `<div class="message ${roleClass}" data-msg-id="${msgId}">
                <div class="message-content">${escapeHtml(m.content)}</div>
                <div class="message-actions">
                    <button class="msg-btn copy-btn" onclick="copyMessage('${msgId}')">📋</button>
                    <button class="msg-btn edit-btn" onclick="editMessageUI('${msgId}')">✏️</button>
                    ${m.role === 'assistant' ? `<button class="msg-btn regen-btn" onclick="regenerateMessage('${msgId}')">🔄</button>` : ''}
                    <button class="msg-btn branch-btn" onclick="branchSession('${sessionId}', '${msgId}')">🌿</button>
                    <button class="msg-btn delete-btn" onclick="deleteMessageUI('${msgId}')">🗑️</button>
                </div>
                <div class="time">${formatTime(m.time_created)}${tokenBadge}</div>
            </div>`;
        });
        document.getElementById('chat-messages').innerHTML = html || '<div style="text-align: center; color: #a2a2a2; padding: 40px;">空会话</div>';
        loadSessions();
    } catch (e) { document.getElementById('chat-messages').innerHTML = `<div class="error">加载失败: ${e.message}</div>`; }
}

function updateTotalTokens() {
    if (!currentSessionId) return;
    fetch(`${API_BASE}/chat/history/${currentSessionId}`).then(function(r){return r.json();}).then(function(msgs){
        var total = 0;
        msgs.forEach(function(m){ if(m.tokens) total += m.tokens; });
        document.getElementById('compress-hint').innerHTML = '💰 ' + total + ' tokens';
    }).catch(function(){});
}

function copyCurrentSessionId() {
    if (!window._currentSessionId) return;
    var input = document.createElement('input');
    input.value = window._currentSessionId;
    input.style.position = 'fixed';
    input.style.opacity = '0';
    document.body.appendChild(input);
    input.select();
    try {
        document.execCommand('copy');
        showToast('✅ Session ID 已复制');
    } catch (e) {
        prompt('手动复制 Session ID:', window._currentSessionId);
    }
    document.body.removeChild(input);
}

async function showContextEditor() {
    const sid = window._currentSessionId;
    if (!sid) { showToast('请先创建或选择会话'); return; }
    try {
        const res = await fetch(`/api/chat/context/${sid}`);
        const data = await res.json();
        const current = data.context || '';
        const newContext = prompt('编辑重要上下文（LLM会自动提取，你也可以手动修改）：', current);
        if (newContext === null) return;
        await fetch(`/api/chat/context/${sid}`, {
            method: 'PUT',
            headers: {'Content-Type': 'application/json'},
            body: JSON.stringify({ context: newContext })
        });
        showToast('重要上下文已更新');
    } catch (e) {
        showToast('加载失败: ' + e.message);
    }
}

function updateSessionDisplay(sid) {
    if (!sid) return;
    document.getElementById('session-id-display').innerHTML = '🆔 ' + sid;
    document.getElementById('session-copy-btn').style.display = 'inline-block';
    document.getElementById('context-btn').style.display = 'inline-block';
    window._currentSessionId = sid;
    setTimeout(function() { updateTotalTokens(); }, 300);
}

async function newSession() {
    currentSessionId = null;
    localStorage.removeItem('chat_session_id');
    document.getElementById('chat-messages').innerHTML = '<div style="text-align: center; color: #a2a2a2; padding: 40px;"><p>开始新对话</p></div>';
    document.getElementById('session-id-display').innerHTML = '💬 待创建';
    document.getElementById('session-copy-btn').style.display = 'none';
    document.getElementById('context-btn').style.display = 'none';
    document.getElementById('compress-hint').innerHTML = '';
    loadSessions();
}

async function sendMessage() {
    const input = document.getElementById('chat-input');
    const message = input.value.trim();
    if (!message) return;
    const useVector = document.getElementById('use-vector').checked;
    const useBM25 = document.getElementById('use-bm25').checked;
    const useHybrid = document.getElementById('use-hybrid').checked;
    const useNone = document.getElementById('use-none').checked;
    let compressMode = document.getElementById('compress-mode').value;
    const compressCount = document.getElementById('compress-count').value;
    if (compressCount) {
        if (compressMode === 'sliding_window') compressMode = 'sliding_window_' + compressCount;
        else if (compressMode === 'token_limit') compressMode = 'token_limit_' + compressCount;
        else if (compressMode === 'summary') compressMode = 'summary_' + compressCount;
        else if (compressMode === 'afm') compressMode = 'afm_' + compressCount;
    }
    const topK = parseInt(document.getElementById('rag-top-k').value) || 5;
    input.value = ''; input.disabled = true; document.getElementById('send-btn').disabled = true;
    const messagesDiv = document.getElementById('chat-messages');
    messagesDiv.innerHTML += `<div class="message user"><div>${escapeHtml(message)}</div><div class="time">${new Date().toLocaleTimeString()}</div></div>`;
    const assistantDiv = document.createElement('div');
    assistantDiv.className = 'message assistant';
    assistantDiv.innerHTML = '<div class="streaming">正在思考...</div>';
    messagesDiv.appendChild(assistantDiv);
    messagesDiv.scrollTop = messagesDiv.scrollHeight;
    let fullReply = ''; let sessionId = currentSessionId; let sourcesCount = 0;
    displayedText = ''; window.typewriterQueue = ''; window.typewriterRunning = false; window.typewriterDone = false;
    try {
        const response = await fetch(`${API_BASE}/chat/stream`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({ session_id: currentSessionId, message: message, use_vector: useNone ? false : (useHybrid ? true : useVector), use_bm25: useNone ? false : (useHybrid ? true : useBM25), top_k: topK, compress_mode: compressMode }) });
        const reader = response.body.getReader(); const decoder = new TextDecoder(); let currentEvent = '';
        while (true) { const { done, value } = await reader.read(); if (done) break; const chunk = decoder.decode(value); const lines = chunk.split('\n');
            for (let i = 0; i < lines.length; i++) { const line = lines[i];
                if (line.startsWith('event:')) { currentEvent = line.slice(6).trim(); continue; }
                if (line.startsWith('data:')) { const data = line.slice(5);
                    if (currentEvent === 'session') { sessionId = data.trim(); updateSessionDisplay(sessionId); currentEvent = ''; }
                    else if (currentEvent === 'mode') { currentEvent = ''; }
                    else if (currentEvent === 'token') { fullReply += data; if (!window.typewriterQueue) window.typewriterQueue = ''; window.typewriterQueue += data; if (!window.typewriterRunning) { window.typewriterRunning = true; typeWriterEffect(assistantDiv, messagesDiv); } currentEvent = ''; }
                    else if (currentEvent === 'done' || data === '[DONE]') { if (sessionId) { currentSessionId = sessionId; updateSessionDisplay(sessionId); localStorage.setItem('chat_session_id', sessionId); } window.typewriterQueue = ''; window.typewriterDone = true;
                        setTimeout(() => { window.typewriterRunning = false; window.typewriterDone = false; var estTokens = Math.ceil(fullReply.length / 4); assistantDiv.innerHTML = `<div>${escapeHtml(fullReply)}</div><div class="time">${new Date().toLocaleTimeString()} <span style="background:#e2e8f0;color:#64748b;border-radius:4px;padding:1px 6px;font-size:10px;margin-left:8px;">~${estTokens}t</span></div>`; if (sourcesCount > 0) assistantDiv.innerHTML += `<div class="sources">参考文档: ${sourcesCount}条</div>`; messagesDiv.scrollTop = messagesDiv.scrollHeight; updateTotalTokens(); loadSessions(); fetchStats(); }, 100); currentEvent = ''; }
                    else if (currentEvent === 'error') { assistantDiv.innerHTML = `<div class="error">${data}</div>`; currentEvent = ''; }
                    else if (data && data !== '[DONE]') { fullReply += data; if (!window.typewriterQueue) window.typewriterQueue = ''; window.typewriterQueue += data; if (!window.typewriterRunning) { window.typewriterRunning = true; typeWriterEffect(assistantDiv, messagesDiv); } }
                }
            }
        }
    } catch (e) { assistantDiv.innerHTML = `<div class="error">发送失败: ${e.message}</div>`; }
    input.disabled = false; document.getElementById('send-btn').disabled = false; input.focus();
}

function typeWriterEffect(assistantDiv, messagesDiv) {
    if (!window.typewriterQueue || window.typewriterQueue.length === 0) { if (window.typewriterDone) return; setTimeout(() => typeWriterEffect(assistantDiv, messagesDiv), 50); return; }
    const charsToShow = Math.min(window.typewriterQueue.length, 2);
    displayedText += window.typewriterQueue.substring(0, charsToShow);
    window.typewriterQueue = window.typewriterQueue.substring(charsToShow);
    assistantDiv.innerHTML = `<div>${escapeHtml(displayedText)}<span class="streaming">▊</span></div>`;
    messagesDiv.scrollTop = messagesDiv.scrollHeight;
    const delay = charsToShow > 0 && window.typewriterQueue.length > 20 ? 20 : 30;
    setTimeout(() => typeWriterEffect(assistantDiv, messagesDiv), delay);
}

async function deleteSession(sessionId) {
    if (!confirm('确定要删除这个会话吗？')) return;
    try {
        await fetch(`${API_BASE}/chat/clear/${sessionId}`, {method: 'POST'});
        if (currentSessionId === sessionId) newSession();
        loadAllSessions(); fetchStats();
    } catch (e) { alert(`删除失败: ${e.message}`); }
}

function copyMessage(msgId) {
    const msgEl = document.querySelector(`.message[data-msg-id="${msgId}"] .message-content`);
    if (msgEl) {
        navigator.clipboard.writeText(msgEl.textContent).then(() => {
            showToast('已复制到剪贴板');
        });
    }
}

async function editMessageUI(msgId) {
    const msgEl = document.querySelector(`.message[data-msg-id="${msgId}"]`);
    const contentEl = msgEl.querySelector('.message-content');
    const originalContent = contentEl.textContent;
    
    const newContent = prompt('编辑消息内容:', originalContent);
    if (newContent === null || newContent === originalContent) return;
    
    try {
        await fetch(`${API_BASE}/chat/message/${msgId}`, {
            method: 'PUT',
            headers: {'Content-Type': 'application/json'},
            body: JSON.stringify({content: newContent})
        });
        contentEl.textContent = newContent;
        showToast('消息已更新');
    } catch (e) {
        alert('更新失败: ' + e.message);
    }
}

async function deleteMessageUI(msgId) {
    if (!confirm('确定删除这条消息？')) return;
    
    try {
        await fetch(`${API_BASE}/chat/message/${msgId}`, {method: 'DELETE'});
        const msgEl = document.querySelector(`.message[data-msg-id="${msgId}"]`);
        if (msgEl) msgEl.remove();
        showToast('消息已删除');
        loadSessions();
    } catch (e) {
        alert('删除失败: ' + e.message);
    }
}

function showToast(message) {
    const toast = document.createElement('div');
    toast.className = 'toast';
    toast.textContent = message;
    document.body.appendChild(toast);
    setTimeout(() => toast.remove(), 2000);
}

function initDarkMode() {
    const savedMode = localStorage.getItem('darkMode');
    if (savedMode === 'true') {
        document.body.classList.add('dark-mode');
    }
}

function toggleDarkMode() {
    document.body.classList.toggle('dark-mode');
    const isDark = document.body.classList.contains('dark-mode');
    localStorage.setItem('darkMode', isDark);
    showToast(isDark ? '已切换到深色模式' : '已切换到浅色模式');
}

async function loadMonitorStats() {
    try {
        const res = await fetch(`${API_BASE}/monitor/stats`);
        const stats = await res.json();
        
        document.getElementById('monitor-total-calls').textContent = stats.total_calls;
        document.getElementById('monitor-total-tokens').textContent = stats.total_tokens;
        document.getElementById('monitor-calls-today').textContent = stats.calls_today;
        document.getElementById('monitor-tokens-today').textContent = stats.tokens_today;
        
        const successRate = stats.total_calls > 0 ? ((stats.success_count / stats.total_calls) * 100).toFixed(1) + '%' : '--';
        document.getElementById('monitor-success-rate').textContent = '成功率: ' + successRate;
        
        const avgDuration = stats.avg_duration_today_ms > 0 ? stats.avg_duration_today_ms + 'ms' : '--';
        document.getElementById('monitor-avg-duration').textContent = '平均耗时: ' + avgDuration;
        
        let typesHtml = '';
        stats.api_types.forEach(t => {
            typesHtml += `<div style="display:flex;justify-content:space-between;padding:12px;background:#f8fafc;border-radius:8px;">
                <span style="font-weight:500;color:#1e40af;">${t.api_type}</span>
                <div style="display:flex;gap:20px;">
                    <span style="color:#64748b;">调用: ${t.call_count}</span>
                    <span style="color:#059669;">Token: ${t.tokens_used}</span>
                    <span style="color:#f59e0b;">平均: ${t.avg_duration_ms}ms</span>
                </div>
            </div>`;
        });
        document.getElementById('monitor-api-types').innerHTML = typesHtml || '<div style="color:#64748b;">暂无数据</div>';
        
        renderMonitorChart(stats);
    } catch (e) {
        console.error('加载监控数据失败:', e);
        showToast('加载监控数据失败');
    }
}

function renderMonitorChart(stats) {
    const canvas = document.getElementById('monitor-chart');
    if (!canvas) return;
    
    if (window._monitorChart) window._monitorChart.destroy();
    
    const ctx = canvas.getContext('2d');
    window._monitorChart = new Chart(ctx, {
        type: 'bar',
        data: {
            labels: stats.api_types.map(t => t.api_type),
            datasets: [{
                label: '调用次数',
                data: stats.api_types.map(t => t.call_count),
                backgroundColor: '#3b82f6'
            }, {
                label: 'Token消耗',
                data: stats.api_types.map(t => t.tokens_used),
                backgroundColor: '#10b981'
            }]
        },
        options: {
            responsive: true,
            plugins: { legend: { position: 'top' } }
        }
    });
}

function handleKeyPress(e) { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage(); } }

function setupSearchModeHandlers() {
    const useVector = document.getElementById('use-vector');
    const useBM25 = document.getElementById('use-bm25');
    const useHybrid = document.getElementById('use-hybrid');
    const useNone = document.getElementById('use-none');
    
    useVector.addEventListener('change', function() {
        if (this.checked) {
            useNone.checked = false;
        }
        updateHybridState();
    });
    
    useBM25.addEventListener('change', function() {
        if (this.checked) {
            useNone.checked = false;
        }
        updateHybridState();
    });
    
    useHybrid.addEventListener('change', function() {
        if (this.checked) {
            useNone.checked = false;
            useVector.checked = true;
            useBM25.checked = true;
        }
    });
    
    useNone.addEventListener('change', function() {
        if (this.checked) {
            useVector.checked = false;
            useBM25.checked = false;
            useHybrid.checked = false;
        }
    });
    
    function updateHybridState() {
        useHybrid.checked = useVector.checked && useBM25.checked;
    }
}

function updateCompressHint() {
    // token显示在compress-hint中，由updateTotalTokens控制
}

function updateCompressUI() {
    const mode = document.getElementById('compress-mode').value;
    const show = mode === 'sliding_window' || mode === 'token_limit' || mode === 'summary' || mode === 'afm' || mode === 'topic';
    document.getElementById('compress-count-wrap').style.display = show ? 'inline' : 'none';
    document.getElementById('compress-count-label').textContent = 
        mode === 'token_limit' ? '限制' :
        mode === 'summary' ? '超过' :
        mode === 'afm' ? '预算' : '保留';
    document.getElementById('compress-count-unit').textContent = 
        mode === 'token_limit' ? 'tokens' :
        mode === 'summary' ? '条触发' :
        mode === 'afm' ? 'tokens' : '条';
    document.getElementById('compress-count').max = mode === 'token_limit' || mode === 'afm' ? 10000 : 100;
}

function updateCompressExp() {
    const threshold = document.getElementById('exp-threshold').value;
    const keepRecent = document.getElementById('exp-keep-recent').value;
    const messages = document.getElementById('exp-messages').value;
    
    document.getElementById('exp-threshold-val').textContent = threshold;
    document.getElementById('exp-keep-recent-val').textContent = keepRecent;
    document.getElementById('exp-messages-val').textContent = messages;
    
    renderCompressVisual(parseInt(messages), parseInt(threshold), parseInt(keepRecent));
}

function renderCompressVisual(total, threshold, keepRecent) {
    const keywords = ['名字', '设定', '角色'];
    let flowHtml = '';
    
    let importantCount = 0;
    let compressCount = 0;
    
    for (let i = 1; i <= total; i++) {
        const isImportant = keywords.some(k => i % 7 === 0);
        const isRecent = i > total - keepRecent;
        
        if (total > threshold) {
            if (isImportant) {
                flowHtml += `<span class="msg-block msg-important">消息${i} ⭐重要</span>`;
                importantCount++;
            } else if (isRecent) {
                flowHtml += `<span class="msg-block msg-recent">消息${i} 📌最近</span>`;
            } else {
                flowHtml += `<span class="msg-block msg-compress">消息${i}</span>`;
                compressCount++;
            }
        } else {
            flowHtml += `<span class="msg-block msg-recent">消息${i}</span>`;
        }
    }
    
    if (total > threshold && compressCount > 3) {
        flowHtml += `<span class="msg-block msg-summary">📝 摘要 (${compressCount}条压缩)</span>`;
    }
    
    document.getElementById('compress-flow').innerHTML = flowHtml;
    
    let resultHtml = '';
    if (total > threshold) {
        const saved = total - (importantCount + 1 + keepRecent);
        resultHtml = `<div style="background:#10b981;color:white;padding:12px 24px;border-radius:8px;">
            压缩生效！原 ${total} 条 → ${importantCount + 1 + keepRecent} 条，节省 ${saved} 条消息
        </div>`;
    } else {
        resultHtml = `<div style="background:#64748b;color:white;padding:12px 24px;border-radius:8px;">
            消息数 ${total} 未超过阈值 ${threshold}，暂不压缩
        </div>`;
    }
    document.getElementById('compress-result').innerHTML = resultHtml;
}

function showModeDetail(mode) {
    document.querySelectorAll('.mode-card').forEach(c => c.classList.remove('active'));
    event.target.closest('.mode-card').classList.add('active');
    
    const details = {
        'none': `<h4 style="color:#1e40af;">不压缩模式</h4>
            <p style="color:#475569;line-height:1.8;">保留所有历史消息，直接发送给LLM。</p>
            <p style="color:#dc2626;">⚠️ 缺点：可能超出token限制，导致API报错或截断。</p>
            <p style="color:#059669;">✅ 适用：短对话、测试场景。</p>`,
        'sliding_window': `<h4 style="color:#1e40af;">滑动窗口模式</h4>
            <p style="color:#475569;line-height:1.8;">只保留最近N条消息，丢弃更早的历史。</p>
            <p style="color:#dc2626;">⚠️ 缺点：丢失重要设定信息。</p>
            <p style="color:#059669;">✅ 适用：不需要记住历史信息的场景。</p>`,
        'token_limit': `<h4 style="color:#1e40af;">Token限制模式</h4>
            <p style="color:#475569;line-height:1.8;">保留前2条消息（关键设定），然后从尾部往前保留直到达到token上限。</p>
            <p style="color:#dc2626;">⚠️ 缺点：可能删除中间消息。</p>
            <p style="color:#059669;">✅ 适用：需要控制API成本的场景。可自定义token上限。</p>`,
        'summary': `<h4 style="color:#1e40af;">摘要压缩模式</h4>
            <p style="color:#475569;line-height:1.8;">将所有历史消息压缩成一条摘要。</p>
            <p style="color:#dc2626;">⚠️ 缺点：摘要可能丢失细节信息。</p>
            <p style="color:#059669;">✅ 适用：长对话、只需保留大意。</p>`,
        'layered': `<h4 style="color:#1e40af;">分层压缩模式（推荐）</h4>
            <p style="color:#475569;line-height:1.8;">
                1️⃣ 检查消息数量是否超过阈值<br>
                2️⃣ 提取包含关键词的消息 → <span style="color:#059669;font-weight:bold">重要消息</span><br>
                3️⃣ 提取最近N条消息 → <span style="color:#3b82f6;font-weight:bold">最近消息</span><br>
                4️⃣ 剩余消息调用LLM生成摘要 → <span style="color:#f59e0b;font-weight:bold">摘要</span><br>
                5️⃣ 组合: 重要 + 摘要 + 最近 → 发送给LLM
            </p>
            <p style="color:#059669;">✅ 适用：生产环境，保护重要信息同时节省token。</p>`,
        'afm': `<h4 style="color:#1e40af;">AFM自适应保真度压缩</h4>
            <p style="color:#475569;line-height:1.8;">
                🔹 LLM 对每条消息分三档：<br>
                <span style="color:#059669;font-weight:bold">Full（完整保留）</span> — 关键设定、用户约束<br>
                <span style="color:#f59e0b;font-weight:bold">Compressed（精简）</span> — 有参考价值的消息，压缩为一句话<br>
                <span style="color:#94a3b8;font-weight:bold">Placeholder（占位）</span> — 闲聊或无关内容，显示"省略X条"
            </p>
            <p style="color:#059669;">✅ 比分层压缩更精细，信息保留更准确。</p>`,
        'topic': `<h4 style="color:#1e40af;">话题分段压缩</h4>
            <p style="color:#475569;line-height:1.8;">
                🔹 LLM 检测话题切换点<br>
                🔹 每一段独立生成摘要<br>
                🔹 保留最近对话完整<br>
                🔹 示例：<br>
                <span style="color:#94a3b8;">[话题] "用户询问Rust的定义和用途"</span><br>
                <span style="color:#94a3b8;">[话题] "用户请求AI扮演电子小狗，取名小爱同学"</span><br>
                <span style="color:#94a3b8;">[话题] "讨论苹果和西瓜的营养价值"</span><br>
                <span style="color:#1e293b;">最近消息...</span>
            </p>
            <p style="color:#059669;">✅ 长对话中按话题组织，比单一摘要更清晰。</p>`
    };
    
    document.getElementById('mode-detail').innerHTML = details[mode] || '';
}

function showResult(elementId, type, message) {
    const el = document.getElementById(elementId);
    if (type === 'loading') el.innerHTML = `<div class="loading">${message}</div>`;
    else if (type === 'error') el.innerHTML = `<div class="error">${message}</div>`;
    else if (type === 'success') el.innerHTML = `<div class="success">${message}</div>`;
}

function escapeHtml(text) { const div = document.createElement('div'); div.textContent = text; return div.innerHTML; }
function formatTime(timestamp) { const date = new Date(timestamp); return date.toLocaleString('zh-CN', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }); }

async function loadLangGraphInfo() {
    try {
        const res = await fetch(`${API_BASE}/langgraph/info`);
        const info = await res.json();
        if (info.parallel_demo) {
            document.getElementById('langgraph-action-bar').querySelector('span').textContent = '点击上方按钮查看图结构';
        }
    } catch (e) { /* ignore */ }
}

let _decomposedData = null;

async function runDecompose() {
    const task = document.getElementById('decompose-input').value.trim();
    if (!task) { showToast('请输入任务'); return; }

    const viz = document.getElementById('langgraph-viz');
    const container = document.getElementById('mermaid-container');
    const results = document.getElementById('langgraph-results');

    viz.style.display = 'block';
    document.getElementById('graph-title').textContent = '🤖 AI 正在拆解任务...';
    document.getElementById('graph-desc').textContent = '';
    container.innerHTML = '<div style="text-align:center;color:#94a3b8;padding:30px;">⏳ LLM 分析中...</div>';
    results.innerHTML = '';

    try {
        const res = await fetch(`${API_BASE}/langgraph/decompose`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ task })
        });
        if (!res.ok) {
            const errData = await res.json().catch(() => ({}));
            throw new Error(errData.error || `服务器返回 ${res.status}`);
        }
        const data = await res.json();

        if (!data.sub_tasks || !data.graph_structure) {
            throw new Error('LLM 返回格式异常，请重试');
        }

        _decomposedData = data;
        document.getElementById('graph-title').textContent = `🤖 任务拆解: ${escapeHtml(data.original_task)}`;
        document.getElementById('graph-desc').textContent = `${data.sub_tasks.length} 个子任务`;
        container.innerHTML = renderGraphHtml(data.graph_structure, {});

        let detailHtml = `<div style="display:flex;justify-content:space-between;align-items:center;margin-top:20px;">`;
        detailHtml += `<h3 style="color:#10b981;margin:0;">📋 子任务列表</h3>`;
        detailHtml += `<button class="btn" onclick="executeDecomposed()" style="background:#10b981;color:white;">▶ 执行所有子任务</button>`;
        detailHtml += `</div>`;
        detailHtml += `<div style="border:1px solid #e2e8f0;border-radius:8px;padding:15px;margin-top:10px;">
            <table style="width:100%;border-collapse:collapse;">
            <thead><tr style="background:#f1f5f9;">
                <th style="padding:8px;border:1px solid #e2e8f0;">子任务</th>
                <th style="padding:8px;border:1px solid #e2e8f0;">描述</th>
                <th style="padding:8px;border:1px solid #e2e8f0;">依赖</th>
            </tr></thead>
            <tbody>`;
        data.sub_tasks.forEach(st => {
            const deps = (st.depends_on && st.depends_on.length) ? st.depends_on.join(', ') : '无';
            detailHtml += `<tr>
                <td style="padding:8px;border:1px solid #e2e8f0;">${escapeHtml(st.name)}</td>
                <td style="padding:8px;border:1px solid #e2e8f0;">${escapeHtml(st.description)}</td>
                <td style="padding:8px;border:1px solid #e2e8f0;">${deps}</td>
            </tr>`;
        });
        detailHtml += `</tbody></table></div>`;
        results.innerHTML = detailHtml;
    } catch (e) {
        container.innerHTML = `<div style="color:#e94560;text-align:center;padding:20px;">失败: ${e.message}</div>`;
    }
}

async function executeDecomposed() {
    if (!_decomposedData) return;
    const results = document.getElementById('langgraph-results');
    results.innerHTML = '<div style="text-align:center;color:#94a3b8;padding:20px;">⏳ 执行中...</div>';

    try {
        const res = await fetch(`${API_BASE}/langgraph/execute`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                task: _decomposedData.original_task,
                sub_tasks: _decomposedData.sub_tasks
            })
        });
        if (!res.ok) {
            const errData = await res.json().catch(() => ({}));
            throw new Error(errData.error || `服务器返回 ${res.status}`);
        }
        const data = await res.json();

        // 更新图上的注解
        const annotations = {};
        (data.execution_results || []).forEach(r => {
            annotations[r.name] = { label: r.output, ms: r.duration_ms };
        });
        const container = document.getElementById('mermaid-container');
        container.innerHTML = renderGraphHtml(_decomposedData.graph_structure, annotations);

        // 显示结果表格（含 token）
        let html = `<h3 style="color:#10b981;margin-top:20px;">✅ 执行完成</h3>`;
        html += `<div style="border:1px solid #e2e8f0;border-radius:8px;padding:15px;">
            <table style="width:100%;border-collapse:collapse;">
            <thead><tr style="background:#f1f5f9;">
                <th style="padding:8px;border:1px solid #e2e8f0;">子任务</th>
                <th style="padding:8px;border:1px solid #e2e8f0;">输出</th>
                <th style="padding:8px;border:1px solid #e2e8f0;">耗时</th>
                <th style="padding:8px;border:1px solid #e2e8f0;">Token</th>
            </tr></thead>
            <tbody>`;
        (data.execution_results || []).forEach(r => {
            html += `<tr>
                <td style="padding:8px;border:1px solid #e2e8f0;">${escapeHtml(r.name)}</td>
                <td style="padding:8px;border:1px solid #e2e8f0;">${escapeHtml(r.output)}</td>
                <td style="padding:8px;border:1px solid #e2e8f0;">${r.duration_ms}ms</td>
                <td style="padding:8px;border:1px solid #e2e8f0;">${r.tokens || '-'}</td>
            </tr>`;
        });
        html += `</tbody></table></div>`;

        // 汇总
        const totalTokens = (data.execution_results || []).reduce((s, r) => s + (r.tokens || 0), 0);
        const totalMs = (data.execution_results || []).reduce((s, r) => s + r.duration_ms, 0);
        html += `<p style="margin-top:10px;color:#64748b;font-size:13px;">总计: ${totalMs}ms | ${totalTokens} tokens</p>`;

        results.innerHTML = html;
    } catch (e) {
        results.innerHTML = `<div style="color:#e94560;text-align:center;padding:20px;">执行失败: ${e.message}</div>`;
    }
}

const LG_MODE = {
    parallel: { name: '并行执行', color: '#667eea', desc: 'FanOut → 3个任务同时跑' },
    conditional: { name: '条件路由', color: '#f5576c', desc: '根据输入长度动态选路径' },
    stream: { name: '流式执行', color: '#4facfe', desc: 'step1→step2→step3 逐步执行' },
};

async function showLangGraphStructure(mode) {
    const input = document.getElementById('langgraph-input').value || '测试输入';
    const container = document.getElementById('mermaid-container');
    const viz = document.getElementById('langgraph-viz');
    const info = LG_MODE[mode];

    document.getElementById('graph-title').textContent = `📐 ${info.name} - 图结构`;
    document.getElementById('graph-desc').textContent = info.desc;
    container.innerHTML = '<div style="text-align:center;color:#94a3b8;padding:20px;">执行中...</div>';
    viz.style.display = 'block';

    try {
        const [strucRes, execRes] = await Promise.all([
            fetch(`${API_BASE}/langgraph/structure`, {
                method: 'POST', headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ mode })
            }),
            fetch(`${API_BASE}/langgraph/${mode}`, {
                method: 'POST', headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ input })
            }),
        ]);

        const strucData = await strucRes.json();
        const execData = await execRes.json();

        const annotations = buildAnnotations(mode, execData);
        container.innerHTML = renderGraphHtml(strucData.structure, annotations);
        document.getElementById('langgraph-results').innerHTML = renderExecResults(mode, execData);
    } catch (e) {
        container.innerHTML = `<div style="color:#e94560;text-align:center;padding:20px;">失败: ${e.message}</div>`;
    }
}

function buildAnnotations(mode, data) {
    const ann = {};
    if (mode === 'parallel') {
        const taskMap = { 'TaskA': 'task_a', 'TaskB': 'task_b', 'TaskC': 'task_c' };
        (data.parallel_tasks || []).forEach(t => {
            const nodeName = taskMap[t.task_name] || t.task_name.toLowerCase();
            ann[nodeName] = { label: t.result, ms: t.duration_ms };
        });
        ann['dispatcher'] = { label: '将任务分发给 3 个并行节点', ms: 0 };
    } else if (mode === 'conditional') {
        ann['analyze'] = { label: `分析输入，长度=${data.input.length}`, ms: 0 };
        const nodeName = data.path_taken;
        ann[nodeName] = { label: data.output, ms: 0 };
    } else if (mode === 'stream') {
        (data || []).forEach(e => {
            if (e.event_type === 'complete' || e.event_type === 'enter') {
                ann[e.node_name] = { label: `执行中`, ms: e.timestamp_ms };
            }
        });
    }
    return ann;
}

function renderExecResults(mode, data) {
    const info = LG_MODE[mode];
    let html = `<h3 style="color:${info.color};margin-top:20px;">${info.name}结果</h3>`;

    if (mode === 'parallel') {
        html += `<div style="border:1px solid #e2e8f0;padding:15px;border-radius:8px;">
            <p><strong>输入：</strong>${escapeHtml(data.input)}</p>
            <p><strong>结果：</strong>${escapeHtml(data.merged_result)}</p>
            <p><strong>总耗时：</strong>${data.total_time_ms}ms | <strong>节省：</strong>${data.time_saved_percent.toFixed(1)}%</p>
            <h4 style="margin-top:10px;">并行任务：</h4>
            <ul>${data.parallel_tasks.map(t => `<li>${t.task_name}: ${t.result} (${t.duration_ms}ms)</li>`).join('')}</ul>
        </div>`;
    } else if (mode === 'conditional') {
        html += `<div style="border:1px solid #e2e8f0;padding:15px;border-radius:8px;">
            <p><strong>输入：</strong>${escapeHtml(data.input)} (长度: ${data.input.length})</p>
            <p><strong>路由决策：</strong>${escapeHtml(data.route_decision)}</p>
            <p><strong>执行路径：</strong>${escapeHtml(data.path_taken)}</p>
            <p><strong>输出：</strong>${escapeHtml(data.output)}</p>
            <h4 style="margin-top:10px;">步骤：</h4>
            <ol>${data.steps.map(s => `<li>${escapeHtml(s)}</li>`).join('')}</ol>
        </div>`;
    } else if (mode === 'stream') {
        html += `<div style="border:1px solid #e2e8f0;padding:15px;border-radius:8px;">
            <p><strong>事件数：</strong>${data.length}</p>
            <table style="width:100%;margin-top:10px;border-collapse:collapse;">
                <thead><tr style="background:#f1f5f9;">
                    <th style="padding:8px;border:1px solid #e2e8f0;">节点</th>
                    <th style="padding:8px;border:1px solid #e2e8f0;">事件</th>
                    <th style="padding:8px;border:1px solid #e2e8f0;">时间(ms)</th>
                </tr></thead>
                <tbody>${data.map(e => `<tr>
                    <td style="padding:8px;border:1px solid #e2e8f0;">${escapeHtml(e.node_name)}</td>
                    <td style="padding:8px;border:1px solid #e2e8f0;">${escapeHtml(e.event_type)}</td>
                    <td style="padding:8px;border:1px solid #e2e8f0;">${e.timestamp_ms}</td>
                </tr>`).join('')}</tbody>
            </table>
        </div>`;
    }
    return html;
}

function renderGraphHtml(structure, annotations) {
    annotations = annotations || {};
    const nodes = structure.nodes || [];
    const edges = structure.edges || [];
    const entry = structure.entry_point || '';
    const nodeColors = {};
    nodes.forEach(n => {
        if (n === entry) nodeColors[n] = '#10b981';
        else nodeColors[n] = '#3b82f6';
    });

    let html = '<div style="display:flex;flex-direction:column;align-items:center;gap:4px;padding:10px;">';
    html += renderGraphNode('START', '#10b981', 'START', null);
    html += renderArrow();
    html += renderFlowLevel(nodes, edges, entry, nodeColors, annotations);
    html += renderArrow();
    html += renderGraphNode('END', '#ef4444', 'END', null);
    html += '</div>';
    return html;
}

function renderFlowLevel(nodes, edges, entry, nodeColors, annotations) {
    const outgoing = {};
    edges.forEach(e => {
        const src = e.source === '__start__' ? 'START' : e.source;
        if (!outgoing[src]) outgoing[src] = [];
        outgoing[src].push(e);
    });

    const visited = new Set();
    const levels = [];
    let current = new Set(['START']);
    while (current.size > 0) {
        const next = new Set();
        const level = [];
        current.forEach(s => {
            if (visited.has(s)) return;
            visited.add(s);
            if (s !== 'START' && s !== 'END') level.push(s);
            const edgesFrom = outgoing[s] || [];
            edgesFrom.forEach(e => {
                if (e.type === 'fanout') {
                    (e.targets || []).forEach(t => {
                        const tn = t === '__end__' ? 'END' : t;
                        if (!visited.has(tn)) next.add(tn);
                    });
                } else if (e.type === 'conditional') {
                    Object.values(e.targets || {}).forEach(t => {
                        const tn = t === '__end__' ? 'END' : t;
                        if (!visited.has(tn)) next.add(tn);
                    });
                } else {
                    const tn = (e.target === '__end__' ? 'END' : e.target);
                    if (!visited.has(tn)) next.add(tn);
                }
            });
        });
        if (level.length > 0) levels.push(level);
        current = next;
    }

    let html = '';
    levels.forEach((level, li) => {
        if (li > 0) html += renderArrow();
        if (level.length === 1) {
            html += renderGraphNode(level[0], nodeColors[level[0]] || '#3b82f6', level[0], annotations[level[0]] || null);
        } else {
            html += '<div style="display:flex;gap:24px;justify-content:center;">';
            level.forEach(n => {
                html += renderGraphNode(n, nodeColors[n] || '#3b82f6', n, annotations[n] || null);
            });
            html += '</div>';
            const edge = outgoing[level[0]] || [];
            if (edge.find(e => e.type === 'fanout')) {
                html += '<div style="font-size:11px;color:#64748b;margin-top:2px;">[FanOut 并行]</div>';
            }
            if (edge.find(e => e.type === 'conditional')) {
                html += '<div style="font-size:11px;color:#64748b;margin-top:2px;">[条件路由]</div>';
            }
        }
    });
    return html;
}

const NODE_ROLES = {
    dispatcher: { icon: '📨', role: '任务分发' },
    task_a: { icon: '📥', role: '数据获取' },
    task_b: { icon: '📄', role: '文档处理' },
    task_c: { icon: '📊', role: '内容分析' },
    analyze: { icon: '🔍', role: '输入分析' },
    quick_process: { icon: '⚡', role: '快速处理' },
    detailed_process: { icon: '📋', role: '详细分析' },
    step1: { icon: '①', role: '第一步' },
    step2: { icon: '②', role: '第二步' },
    step3: { icon: '③', role: '第三步' },
};

function renderGraphNode(name, color, label, annotation) {
    const bg = color + '22';
    const border = color;
    const role = NODE_ROLES[name];
    const icon = role ? role.icon + ' ' : '';
    const roleText = role ? `<div style="font-size:11px;color:#64748b;margin-top:2px;">${role.role}</div>` : '';
    let html = `<div style="display:inline-flex;flex-direction:column;align-items:center;min-width:140px;">`;
    html += `<div style="background:${bg};border:2px solid ${border};border-radius:10px;padding:8px 16px;text-align:center;">`;
    html += `<div style="font-weight:bold;font-size:14px;color:#1e293b;">${icon}${label}</div>`;
    html += roleText;
    html += `</div>`;
    if (annotation) {
        const msText = annotation.ms ? ` (${annotation.ms}ms)` : '';
        html += `<div style="margin-top:4px;font-size:11px;color:#475569;background:#f0fdf4;border:1px solid #bbf7d0;border-radius:6px;padding:4px 10px;max-width:200px;text-align:center;word-break:break-all;">${escapeHtml(annotation.label)}${msText}</div>`;
    }
    html += '</div>';
    return html;
}

function renderArrow() {
    return '<div style="color:#94a3b8;font-size:18px;">↓</div>';
}



async function regenerateMessage(msgId) {
    if (!confirm('确定重新生成这条AI回复？')) return;
    try {
        showToast('正在重新生成...');
        const res = await fetch(`${API_BASE}/chat/message/${msgId}/regenerate`, {method: 'POST'});
        const data = await res.json();
        if (data.message_id) {
            const msgEl = document.querySelector(`.message[data-msg-id="${msgId}"]`);
            if (msgEl) {
                msgEl.querySelector('.message-content').textContent = data.reply;
                msgEl.dataset.msgId = data.message_id;
            }
            showToast('已重新生成');
            loadSessions();
        } else {
            showToast('重新生成失败');
        }
    } catch (e) { showToast('重新生成失败: ' + e.message); }
}

async function exportSession(sessionId) {
    try {
        const res = await fetch(`${API_BASE}/chat/session/${sessionId}/export`);
        const data = await res.json();
        const json = JSON.stringify(data, null, 2);
        const blob = new Blob([json], {type: 'application/json'});
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = `session_${sessionId}.json`;
        a.click();
        URL.revokeObjectURL(url);
        showToast('会话已导出');
    } catch (e) { showToast('导出失败: ' + e.message); }
}

async function branchSession(sessionId, fromMsgId) {
    if (!confirm('确定从此消息创建分支？')) return;
    try {
        const res = await fetch(`${API_BASE}/chat/session/branch`, {
            method: 'POST',
            headers: {'Content-Type': 'application/json'},
            body: JSON.stringify({session_id: sessionId, from_message_id: fromMsgId})
        });
        const data = await res.json();
        if (data.new_session_id) {
            showToast(`已创建分支: ${data.title}`);
            currentSessionId = data.new_session_id;
            loadSession(data.new_session_id);
            loadSessions();
        }
    } catch (e) { showToast('创建分支失败: ' + e.message); }
}

async function importSession() {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = '.json';
    input.onchange = async (e) => {
        const file = e.target.files[0];
        if (!file) return;
        try {
            const text = await file.text();
            const data = JSON.parse(text);
            const res = await fetch(`${API_BASE}/chat/session/import`, {
                method: 'POST',
                headers: {'Content-Type': 'application/json'},
                body: JSON.stringify(data)
            });
            const result = await res.json();
            if (result.success) {
                showToast('会话导入成功');
                loadSessions(); loadAllSessions();
            } else {
                showToast('导入失败');
            }
        } catch (e) { showToast('导入失败: ' + e.message); }
    };
    input.click();
}

async function searchSessions(query) {
    if (!query) return;
    try {
        const res = await fetch(`${API_BASE}/chat/sessions/search`, {
            method: 'POST',
            headers: {'Content-Type': 'application/json'},
            body: JSON.stringify({query})
        });
        const sessions = await res.json();
        let html = '<div style="display: grid; gap: 10px;">';
        sessions.forEach(s => {
            html += `<div class="session-item" onclick="loadSession('${s.session_id}'); showTab('chat');">
                <div style="font-weight:500;">${escapeHtml(s.title)}</div>
                <div class="time">${formatTime(s.created_at)} | ${s.message_count}条</div>
            </div>`;
        });
        html += '</div>';
        document.getElementById('sessions-list').innerHTML = html || '<div style="color:#64748b;">无匹配会话</div>';
    } catch (e) { showToast('搜索失败: ' + e.message); }
}

async function batchDeleteDocuments() {
    const checkboxes = document.querySelectorAll('.doc-checkbox:checked');
    const ids = Array.from(checkboxes).map(cb => cb.value);
    if (ids.length === 0) { showToast('请选择要删除的文档'); return; }
    if (!confirm(`确定删除 ${ids.length} 个文档？`)) return;
    try {
        const res = await fetch(`${API_BASE}/documents/batch-delete`, {
            method: 'POST',
            headers: {'Content-Type': 'application/json'},
            body: JSON.stringify({parent_ids: ids})
        });
        const data = await res.json();
        showToast(data.message);
        loadDocuments(); fetchStats();
    } catch (e) { showToast('删除失败: ' + e.message); }
}

async function addDocumentTags(parentId) {
    const tags = prompt('输入标签（逗号分隔）:');
    if (!tags) return;
    const tagList = tags.split(',').map(t => t.trim()).filter(t => t);
    try {
        await fetch(`${API_BASE}/documents/tags`, {
            method: 'POST',
            headers: {'Content-Type': 'application/json'},
            body: JSON.stringify({parent_id: parentId, tags: tagList})
        });
        showToast('标签已添加');
        loadDocuments();
    } catch (e) { showToast('添加标签失败: ' + e.message); }
}

async function loadDocumentsByTag(tag) {
    try {
        const res = await fetch(`${API_BASE}/documents/tag/${encodeURIComponent(tag)}`);
        const documents = await res.json();
        let html = `<h3 style="color:#1e40af;margin-bottom:15px;">标签: ${escapeHtml(tag)} (${documents.length}个文档)</h3>`;
        html += '<table style="width:100%;border-collapse:collapse;"><tbody>';
        documents.forEach(doc => {
            html += `<tr><td style="padding:12px;border:1px solid #e2e8f0;">${escapeHtml(doc.title)}</td>
                <td style="padding:12px;border:1px solid #e2e8f0;text-align:center;">${doc.chunk_count}</td>
                <td style="padding:12px;border:1px solid #e2e8f0;"><button class="btn btn-small" onclick="previewDocument('${doc.id}')">预览</button></td></tr>`;
        });
        html += '</tbody></table>';
        document.getElementById('documents-list').innerHTML = html || '<div style="color:#64748b;">无文档</div>';
    } catch (e) { showToast('加载失败: ' + e.message); }
}

let _agentPlanData = null;

async function runAgentPlan() {
    const task = document.getElementById('agent-input').value.trim();
    if (!task) { showToast('请输入任务'); return; }
    const viz = document.getElementById('agent-viz');
    const container = document.getElementById('agent-container');
    const detail = document.getElementById('agent-plan-detail');
    const results = document.getElementById('agent-results');

    viz.style.display = 'block'; detail.style.display = 'none';
    document.getElementById('agent-graph-title').textContent = '🤖 规划中...';
    container.innerHTML = '<div style="text-align:center;color:#94a3b8;padding:30px;">⏳ LLM 规划中...</div>';
    results.innerHTML = '';

    try {
        const res = await fetch(`${API_BASE}/agent/plan`, {
            method: 'POST', headers: {'Content-Type':'application/json'},
            body: JSON.stringify({task})
        });
        if (!res.ok) { const e=await res.json().catch(()=>({})); throw new Error(e.error||`${res.status}`); }
        const data = await res.json();
        _agentPlanData = data;

        document.getElementById('agent-graph-title').textContent = `📋 ${escapeHtml(data.original_task)}`;
        container.innerHTML = renderGraphHtml(data.graph_structure, {});

        let html = `<div style="border:1px solid #e2e8f0;border-radius:8px;padding:15px;margin-top:10px;">
            <table style="width:100%;border-collapse:collapse;">
            <thead><tr style="background:#f5f3ff;">
                <th style="padding:8px;border:1px solid #e2e8f0;">任务</th>
                <th style="padding:8px;border:1px solid #e2e8f0;">工具</th>
                <th style="padding:8px;border:1px solid #e2e8f0;">依赖</th>
                <th style="padding:8px;border:1px solid #e2e8f0;">输入说明</th>
            </tr></thead>
            <tbody>`;
        data.tasks.forEach(t => {
            html += `<tr><td style="padding:8px;border:1px solid #e2e8f0;"><strong>${escapeHtml(t.name)}</strong><br><span style="font-size:12px;color:#64748b;">${escapeHtml(t.description)}</span></td>
                <td style="padding:8px;border:1px solid #e2e8f0;"><span style="background:#ede9fe;padding:2px 8px;border-radius:4px;font-size:12px;">${escapeHtml(t.tool)}</span></td>
                <td style="padding:8px;border:1px solid #e2e8f0;font-size:12px;">${(t.depends_on||[]).join(', ') || '无'}</td>
                <td style="padding:8px;border:1px solid #e2e8f0;font-size:12px;color:#475569;">${escapeHtml(t.input_template)}</td></tr>`;
        });
        html += `</tbody></table></div>`;
        html += `<button class="btn" onclick="agentStepExecute()" style="background:#8b5cf6;color:white;margin-top:10px;padding:12px;width:100%;">▶ 开始执行</button>`;
        results.innerHTML = html;
        detail.style.display = 'block';
    } catch (e) {
        container.innerHTML = `<div style="color:#e94560;text-align:center;padding:20px;">${e.message}</div>`;
    }
}

let _agentSessionId = null;
let _agentAllResults = [];

async function agentStepExecute() {
    if (!_agentPlanData) { alert('请先规划'); return; }
    _agentSessionId = null;
    _agentAllResults = [];
    await agentFetchAndShow(true);
}

async function agentNextBatch() {
    document.querySelectorAll('#agent-results button').forEach(b => b.remove());
    const el = document.createElement('div');
    el.id = 'batch-loading';
    el.style.cssText = 'text-align:center;padding:20px;color:#8b5cf6;';
    el.textContent = '⏳ 执行中...';
    document.getElementById('agent-results').appendChild(el);
    await agentFetchAndShow(false);
}

async function agentFetchAndShow(isFirst) {
    const resDiv = document.getElementById('agent-results');
    if (isFirst) {
        resDiv.innerHTML = '<div style="text-align:center;padding:30px;color:#8b5cf6;">⏳ 执行中...</div>';
    }
    // 非第一步不重置内容，保留已有结果

    try {
        const url = isFirst ? '/api/agent/execute' : '/api/agent/next';
        const body = isFirst
            ? JSON.stringify({task: _agentPlanData.original_task, agent_tasks: _agentPlanData.tasks})
            : JSON.stringify({session_id: _agentSessionId});

        const res = await fetch(url, {method:'POST', headers:{'Content-Type':'application/json'}, body});
        const data = await res.json();
        if (!res.ok) { throw new Error(data.error || '失败'); }

        if (isFirst) _agentSessionId = data.session_id;
        (data.results || []).forEach(r => _agentAllResults.push(r));

        const annotations = {};
        _agentAllResults.forEach(r => { annotations[r.task_name] = {label: r.output.substring(0,30), ms: 0}; });
        document.getElementById('agent-container').innerHTML = renderGraphHtml(_agentPlanData.graph_structure, annotations);

        let html = '<div id="agent-results-table" style="border:1px solid #e2e8f0;border-radius:8px;padding:15px;margin-top:10px;">';
        html += '<table style="width:100%"><thead><tr style="background:#f5f3ff;"><th>任务</th><th>输出</th><th style="width:100px;">消耗</th></tr></thead><tbody>';
        _agentAllResults.forEach(r => {
            html += '<tr><td style="padding:8px;font-weight:bold;">' + escapeHtml(r.task_name) + '</td>';
            html += '<td style="padding:8px;font-size:13px;">' + escapeHtml(r.output) + '</td>';
            html += '<td style="padding:4px;font-size:11px;color:#94a3b8;text-align:right;">' + (r.duration_ms||0) + 'ms | ' + (r.tokens||0) + 't</td></tr>';
        });
        html += '</tbody></table>';

        // 替换或追加到已有结果
        if (isFirst) {
            html += '<div style="padding:10px;text-align:center;">'
                + (data.has_next
                    ? '<button class="btn" onclick="agentNextBatch()" style="background:#8b5cf6;color:white;width:100%;padding:12px;">▶ 下一步 (' + _agentAllResults.length + '/' + _agentPlanData.tasks.length + ')</button>'
                    : '<span style="color:#10b981;font-weight:bold;">✅ 全部完成</span>')
                + '</div></div>';
            resDiv.innerHTML = html;
        } else {
            // 移除加载指示器和旧按钮
            document.querySelectorAll('#batch-loading, #agent-results .btn, #agent-results span').forEach(el => el.remove());
            // 追加新按钮
            let footer = document.createElement('div');
            footer.style.cssText = 'padding:10px;text-align:center;';
            if (data.has_next) {
                footer.innerHTML = '<button class="btn" onclick="agentNextBatch()" style="background:#8b5cf6;color:white;width:100%;padding:12px;">▶ 下一步 (' + _agentAllResults.length + '/' + _agentPlanData.tasks.length + ')</button>';
            } else {
                footer.innerHTML = '<span style="color:#10b981;font-weight:bold;">✅ 全部完成</span>';
            }
            document.getElementById('agent-results-table').appendChild(footer);
        }
    } catch (e) {
        resDiv.innerHTML = '<div style="color:#e94560;padding:20px;">❌ ' + escapeHtml(e.message) + '</div>';
    }
}

document.addEventListener('DOMContentLoaded', function() {
    const uploadArea = document.getElementById('upload-area');
    const fileInput = document.getElementById('file-input');
    uploadArea.addEventListener('click', () => fileInput.click());
    uploadArea.addEventListener('dragover', (e) => { e.preventDefault(); uploadArea.classList.add('dragover'); });
    uploadArea.addEventListener('dragleave', () => uploadArea.classList.remove('dragover'));
    uploadArea.addEventListener('drop', (e) => { 
        e.preventDefault(); 
        uploadArea.classList.remove('dragover'); 
        const files = e.dataTransfer.files;
        if (files.length > 1) {
            if (confirm(`检测到 ${files.length} 个文件，是否批量上传？`)) {
                uploadMultipleFiles(files);
            } else {
                uploadFile(files[0]);
            }
        } else if (files.length > 0) {
            uploadFile(files[0]);
        }
    });
    fileInput.addEventListener('change', (e) => { 
        const files = e.target.files;
        if (files.length > 1) {
            uploadMultipleFiles(files);
        } else if (files.length > 0) {
            uploadFile(files[0]);
        }
    });
    
    setupSearchModeHandlers();
    document.getElementById('compress-mode').addEventListener('change', updateCompressHint);
    updateCompressHint();
    
    document.getElementById('bm25-query').addEventListener('keypress', (e) => { if (e.key === 'Enter') bm25Search(); });
    document.getElementById('vector-query').addEventListener('keypress', (e) => { if (e.key === 'Enter') vectorSearch(); });
    document.getElementById('compare-query').addEventListener('keypress', (e) => { if (e.key === 'Enter') compareSearch(); });
    
    fetchStats();
    
    const savedSessionId = localStorage.getItem('chat_session_id');
    if (savedSessionId) { currentSessionId = savedSessionId; loadSession(savedSessionId); }
    
    loadLangGraphInfo();
    
    initDarkMode();
});