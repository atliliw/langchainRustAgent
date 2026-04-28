const API_BASE = '/api';
let currentSessionId = null;
let displayedText = '';

async function fetchStats() {
    try {
        const res = await fetch(`${API_BASE}/stats`);
        const data = await res.json();
        document.getElementById('stats-loading').classList.add('hidden');
        document.getElementById('stats-content').classList.remove('hidden');
        document.getElementById('total-docs').textContent = data.total_documents;
        document.getElementById('vector-size').textContent = data.vector_size;
        document.getElementById('bm25-chunks').textContent = data.bm25_chunks;
        document.getElementById('sessions-count').textContent = data.conversation_sessions || 0;
    } catch (e) {
        document.getElementById('stats-loading').innerHTML = `<div class="error">加载失败: ${e.message}</div>`;
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
}

async function loadDocuments() {
    const listEl = document.getElementById('documents-list');
    listEl.innerHTML = '<div class="loading">加载文档列表...</div>';
    try {
        const res = await fetch(`${API_BASE}/documents`);
        const documents = await res.json();
        if (documents.length === 0) { listEl.innerHTML = '<p style="color: #666;">暂无文档，请先上传</p>'; return; }
        let html = '<table style="width: 100%; border-collapse: collapse;"><thead><tr style="background: rgba(255,255,255,0.1);">';
        html += '<th style="padding: 10px; border: 1px solid rgba(255,255,255,0.1);">文档标题</th><th style="padding: 10px; border: 1px solid rgba(255,255,255,0.1);">Chunk数量</th><th style="padding: 10px; border: 1px solid rgba(255,255,255,0.1);">内容预览</th><th style="padding: 10px; border: 1px solid rgba(255,255,255,0.1);">操作</th></tr></thead><tbody>';
        documents.forEach(doc => {
            html += `<tr><td style="padding: 10px; border: 1px solid rgba(255,255,255,0.1);">${escapeHtml(doc.title)}</td><td style="padding: 10px; border: 1px solid rgba(255,255,255,0.1); text-align: center;">${doc.chunk_count}</td><td style="padding: 10px; border: 1px solid rgba(255,255,255,0.1); color: #a2a2a2; font-size: 12px;">${escapeHtml(doc.content_preview)}...</td><td style="padding: 10px; border: 1px solid rgba(255,255,255,0.1);"><button class="btn btn-danger btn-small" onclick="deleteDocument('${doc.id}', '${escapeHtml(doc.title)}')">删除</button></td></tr>`;
        });
        html += '</tbody></table>';
        listEl.innerHTML = html;
    } catch (e) { listEl.innerHTML = `<div class="error">加载失败: ${e.message}</div>`; }
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

async function bm25Search() {
    const query = document.getElementById('bm25-query').value.trim();
    const topK = parseInt(document.getElementById('bm25-top-k').value) || 5;
    if (!query) { showResult('bm25-results', 'error', '请输入搜索内容'); return; }
    showResult('bm25-results', 'loading', '正在搜索...');
    try {
        const res = await fetch(`${API_BASE}/search/bm25`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({query, top_k: topK}) });
        const data = await res.json();
        document.getElementById('bm25-results').innerHTML = `<h3 style="color: #4caf50; margin-top: 15px;">BM25检索 (${data.total_count}条)</h3>${renderResults(data.results)}`;
    } catch (e) { showResult('bm25-results', 'error', `搜索失败: ${e.message}`); }
}

async function vectorSearch() {
    const query = document.getElementById('vector-query').value.trim();
    const topK = parseInt(document.getElementById('vector-top-k').value) || 5;
    if (!query) { showResult('vector-results', 'error', '请输入搜索内容'); return; }
    showResult('vector-results', 'loading', '正在搜索...');
    try {
        const res = await fetch(`${API_BASE}/search/vector`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({query, top_k: topK}) });
        const data = await res.json();
        document.getElementById('vector-results').innerHTML = `<h3 style="color: #e94560; margin-top: 15px;">向量检索 (${data.total_count}条)</h3>${renderResults(data.results)}`;
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
        alert('已复制到剪贴板');
    }).catch(err => {
        console.error('复制失败:', err);
    });
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
        let html = '';
        sessions.forEach(s => {
            const isActive = s.session_id === currentSessionId;
            html += `<div class="session-item ${isActive ? 'active' : ''}" onclick="loadSession('${s.session_id}')"><div>${s.preview || '新对话'}</div><div class="time">${formatTime(s.created_at)} (${s.message_count}条)</div></div>`;
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
    try {
        const res = await fetch(`${API_BASE}/chat/history/${sessionId}`);
        const messages = await res.json();
        let html = '';
        messages.forEach(m => { const roleClass = m.role === 'user' ? 'user' : 'assistant'; html += `<div class="message ${roleClass}"><div>${escapeHtml(m.content)}</div><div class="time">${formatTime(m.timestamp)}</div></div>`; });
        document.getElementById('chat-messages').innerHTML = html || '<div style="text-align: center; color: #a2a2a2; padding: 40px;">空会话</div>';
        loadSessions();
    } catch (e) { document.getElementById('chat-messages').innerHTML = `<div class="error">加载失败: ${e.message}</div>`; }
}

async function newSession() {
    currentSessionId = null;
    localStorage.removeItem('chat_session_id');
    document.getElementById('chat-messages').innerHTML = '<div style="text-align: center; color: #a2a2a2; padding: 40px;"><p>开始新对话</p></div>';
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
    const compressMode = document.getElementById('compress-mode').value;
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
                    if (currentEvent === 'session') { sessionId = data.trim(); currentEvent = ''; }
                    else if (currentEvent === 'mode') { currentEvent = ''; }
                    else if (currentEvent === 'token') { fullReply += data; if (!window.typewriterQueue) window.typewriterQueue = ''; window.typewriterQueue += data; if (!window.typewriterRunning) { window.typewriterRunning = true; typeWriterEffect(assistantDiv, messagesDiv); } currentEvent = ''; }
                    else if (currentEvent === 'done' || data === '[DONE]') { localStorage.setItem('chat_session_id', sessionId); currentSessionId = sessionId; window.typewriterQueue = ''; window.typewriterDone = true;
                        setTimeout(() => { window.typewriterRunning = false; window.typewriterDone = false; assistantDiv.innerHTML = `<div>${escapeHtml(fullReply)}</div><div class="time">${new Date().toLocaleTimeString()}</div>`; if (sourcesCount > 0) assistantDiv.innerHTML += `<div class="sources">参考文档: ${sourcesCount}条</div>`; messagesDiv.scrollTop = messagesDiv.scrollHeight; loadSessions(); fetchStats(); }, 100); currentEvent = ''; }
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
    const mode = document.getElementById('compress-mode').value;
    const hintEl = document.getElementById('compress-hint');
    const hints = { 'none': '⚠️ 可能超出token限制', 'sliding_window': '✅ 简单高效，但丢失早期设定', 'token_limit': '✅ 保护前N条关键设定', 'summary': '✅ 保留语义，调用LLM生成摘要', 'layered': '✅ 最完整，保护重要消息' };
    hintEl.textContent = hints[mode] || '';
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
        document.getElementById('graph-structure').textContent = JSON.stringify(info, null, 2);
    } catch (e) { document.getElementById('graph-structure').textContent = `加载失败: ${e.message}`; }
}

async function runParallelDemo() {
    const input = document.getElementById('langgraph-input').value || '测试输入';
    showResult('langgraph-results', 'loading', '正在执行并行任务...');
    try {
        const res = await fetch(`${API_BASE}/langgraph/parallel`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({input}) });
        const data = await res.json();
        document.getElementById('langgraph-results').innerHTML = `<h3 style="color: #e94560;">并行执行结果</h3><div style="background: rgba(255,255,255,0.05); padding: 15px; border-radius: 8px;"><p><strong>输入：</strong>${escapeHtml(data.input)}</p><p><strong>合并结果：</strong>${escapeHtml(data.merged_result)}</p><p><strong>总耗时：</strong>${data.total_time_ms}ms</p><p><strong>时间节省：</strong>${data.time_saved_percent.toFixed(1)}%</p><h4 style="margin-top: 15px;">并行任务结果：</h4><ul>${data.parallel_tasks.map(t => `<li>${t.task_name}: ${t.result} (${t.duration_ms}ms)</li>`).join('')}</ul></div>`;
    } catch (e) { showResult('langgraph-results', 'error', `执行失败: ${e.message}`); }
}

async function runConditionalDemo() {
    const input = document.getElementById('langgraph-input').value || '测试';
    showResult('langgraph-results', 'loading', '正在执行条件路由...');
    try {
        const res = await fetch(`${API_BASE}/langgraph/conditional`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({input}) });
        const data = await res.json();
        document.getElementById('langgraph-results').innerHTML = `<h3 style="color: #c73e54;">条件路由结果</h3><div style="background: rgba(255,255,255,0.05); padding: 15px; border-radius: 8px;"><p><strong>输入：</strong>${escapeHtml(data.input)} (长度: ${data.input.length})</p><p><strong>路由决策：</strong>${escapeHtml(data.route_decision)}</p><p><strong>执行路径：</strong>${escapeHtml(data.path_taken)}</p><p><strong>输出：</strong>${escapeHtml(data.output)}</p><h4 style="margin-top: 15px;">执行步骤：</h4><ol>${data.steps.map(s => `<li>${escapeHtml(s)}</li>`).join('')}</ol></div>`;
    } catch (e) { showResult('langgraph-results', 'error', `执行失败: ${e.message}`); }
}

async function runStreamDemo() {
    const input = document.getElementById('langgraph-input').value || '测试输入';
    showResult('langgraph-results', 'loading', '正在执行流式演示...');
    try {
        const res = await fetch(`${API_BASE}/langgraph/stream`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({input}) });
        const data = await res.json();
        document.getElementById('langgraph-results').innerHTML = `<h3 style="color: #4caf50;">流式执行事件</h3><div style="background: rgba(255,255,255,0.05); padding: 15px; border-radius: 8px;"><p><strong>事件数量：</strong>${data.length}</p><table style="width: 100%; margin-top: 15px;"><thead><tr style="background: rgba(255,255,255,0.1);"><th style="padding: 8px; border: 1px solid rgba(255,255,255,0.1);">节点</th><th style="padding: 8px; border: 1px solid rgba(255,255,255,0.1);">事件类型</th><th style="padding: 8px; border: 1px solid rgba(255,255,255,0.1);">时间(ms)</th></tr></thead><tbody>${data.map(e => `<tr><td style="padding: 8px; border: 1px solid rgba(255,255,255,0.1);">${escapeHtml(e.node_name)}</td><td style="padding: 8px; border: 1px solid rgba(255,255,255,0.1);">${escapeHtml(e.event_type)}</td><td style="padding: 8px; border: 1px solid rgba(255,255,255,0.1);">${e.timestamp_ms}</td></tr>`).join('')}</tbody></table></div>`;
    } catch (e) { showResult('langgraph-results', 'error', `执行失败: ${e.message}`); }
}

document.addEventListener('DOMContentLoaded', function() {
    const uploadArea = document.getElementById('upload-area');
    const fileInput = document.getElementById('file-input');
    uploadArea.addEventListener('click', () => fileInput.click());
    uploadArea.addEventListener('dragover', (e) => { e.preventDefault(); uploadArea.classList.add('dragover'); });
    uploadArea.addEventListener('dragleave', () => uploadArea.classList.remove('dragover'));
    uploadArea.addEventListener('drop', (e) => { e.preventDefault(); uploadArea.classList.remove('dragover'); if (e.dataTransfer.files.length > 0) uploadFile(e.dataTransfer.files[0]); });
    fileInput.addEventListener('change', (e) => { if (e.target.files.length > 0) uploadFile(e.target.files[0]); });
    
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
});