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
        // 闈欓粯澶勭悊
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
    listEl.innerHTML = '<div class="loading">鍔犺浇鏂囨。鍒楄〃...</div>';
    try {
        // 鍚屾椂鍔犺浇鏅€氭枃妗ｅ拰 PageIndex 鏂囨。
        const [docRes, piRes] = await Promise.all([
            fetch(`${API_BASE}/documents`),
            fetch(`${API_BASE}/documents/pageindex/list`)
        ]);
        const documents = await docRes.json();
        const piDocs = await piRes.json();
        
        if (documents.length === 0 && piDocs.length === 0) {
            listEl.innerHTML = '<p style="color: #666;">鏆傛棤鏂囨。锛岃鍏堜笂浼?/p>';
            return;
        }
        
        let html = '<div style="margin-bottom:15px;display:flex;gap:10px;">';
        html += `<button class="btn btn-small btn-danger" onclick="batchDeleteDocuments()">馃棏锔?鎵归噺鍒犻櫎</button>`;
        html += `<button class="btn btn-small" onclick="importSession()">馃摜 瀵煎叆浼氳瘽</button>`;
        html += '</div>';
        
        // PageIndex 鏂囨。
        if (piDocs.length > 0) {
            html += '<h3 style="color:#6366f1;margin:15px 0 10px 0;">馃搼 PageIndex 鏂囨。鏍?/h3>';
            html += '<table style="width:100%;border-collapse:collapse;margin-bottom:20px;"><thead><tr style="background:#f5f3ff;">';
            html += '<th style="padding:12px;border:1px solid #e0e7ff;">鏂囨。鏍囬</th><th style="padding:12px;border:1px solid #e0e7ff;width:100px;">绫诲瀷</th><th style="padding:12px;border:1px solid #e0e7ff;width:160px;">鎿嶄綔</th></tr></thead><tbody>';
            piDocs.forEach(doc => {
                html += `<tr>
                    <td style="padding:12px;border:1px solid #e0e7ff;">馃搫 ${escapeHtml(doc.title)}</td>
                    <td style="padding:12px;border:1px solid #e0e7ff;text-align:center;"><span style="background:#6366f1;color:white;padding:2px 8px;border-radius:4px;font-size:11px;">PageIndex</span></td>
                    <td style="padding:12px;border:1px solid #e0e7ff;">
                        <button class="btn btn-small" onclick="previewPageIndexTree('${doc.id}', '${escapeHtml(doc.title)}')">馃搨 鏌ョ湅鏍?/button>
                        <button class="btn btn-small btn-danger" onclick="deletePageIndexDoc('${doc.id}', '${escapeHtml(doc.title)}')">鍒犻櫎</button>
                    </td>
                </tr>`;
            });
            html += '</tbody></table>';
        }
        
        // 鏅€氭枃妗ｏ紙鍚戦噺/BM25锛?
        if (documents.length > 0) {
            html += '<h3 style="color:#1e40af;margin:15px 0 10px 0;">馃搫 鏂囨。锛堝悜閲?BM25锛?/h3>';
            html += '<table style="width: 100%; border-collapse: collapse;"><thead><tr style="background: #f1f5f9;">';
            html += '<th style="padding: 12px; border: 1px solid #e2e8f0;width:40px;"><input type="checkbox" onchange="toggleAllDocs(this)"></th>';
            html += '<th style="padding: 12px; border: 1px solid #e2e8f0;">鏂囨。鏍囬</th><th style="padding: 12px; border: 1px solid #e2e8f0;">Chunk鏁伴噺</th><th style="padding: 12px; border: 1px solid #e2e8f0;">鍐呭棰勮</th><th style="padding: 12px; border: 1px solid #e2e8f0;">鎿嶄綔</th></tr></thead><tbody>';
            documents.forEach(doc => {
                html += `<tr>
                    <td style="padding: 12px; border: 1px solid #e2e8f0;text-align:center;"><input type="checkbox" class="doc-checkbox" value="${doc.id}"></td>
                    <td style="padding: 12px; border: 1px solid #e2e8f0;">${escapeHtml(doc.title)}</td>
                    <td style="padding: 12px; border: 1px solid #e2e8f0; text-align: center;">${doc.chunk_count}</td>
                    <td style="padding: 12px; border: 1px solid #e2e8f0; color: #64748b; font-size: 13px;">${escapeHtml(doc.content_preview)}...</td>
                    <td style="padding: 12px; border: 1px solid #e2e8f0;">
                        <button class="btn btn-small" onclick="previewDocument('${doc.id}', '${escapeHtml(doc.title)}')">棰勮</button>
                        <button class="btn btn-small" onclick="addDocumentTags('${doc.id}')">馃彿锔?/button>
                        <button class="btn btn-small btn-danger" onclick="deleteDocument('${doc.id}', '${escapeHtml(doc.title)}')">鍒犻櫎</button>
                    </td>
                </tr>`;
            });
            html += '</tbody></table>';
        }
        
        listEl.innerHTML = html;
    } catch (e) { listEl.innerHTML = `<div class="error">鍔犺浇澶辫触: ${e.message}</div>`; }
}

function toggleAllDocs(cb) {
    document.querySelectorAll('.doc-checkbox').forEach(c => c.checked = cb.checked);
}

async function previewDocument(parentId, title) {
    try {
        const res = await fetch(`${API_BASE}/documents/${encodeURIComponent(title)}/chunks`);
        const chunks = await res.json();
        
        const modal = document.getElementById('detail-modal');
        const contentDiv = document.getElementById('detail-content');
        
        let html = `<h3 style="color:#1e40af;margin-bottom:16px;">馃搫 ${escapeHtml(title)} (${chunks.length} chunks)</h3>`;
        html += '<div style="margin-bottom:10px;display:flex;gap:10px;align-items:center;">';
        html += `<span style="font-size:12px;color:#64748b;">鍒囧垎绛栫暐: 浠庡悜閲忓簱鐩存帴鏌ヨ</span>`;
        html += '</div>';
        html += '<div style="display:grid;gap:12px;max-height:70vh;overflow-y:auto;">';
        chunks.forEach((chunk, idx) => {
            html += `<div class="chunk-card">
                <div class="chunk-header">
                    <span style="background:#1e40af;color:white;padding:4px 10px;border-radius:4px;font-size:12px;">Chunk ${idx + 1}</span>
                    <span style="font-size:11px;color:#94a3b8;">${chunk.content.length} chars</span>
                </div>
                <div class="chunk-content" style="font-size:13px;line-height:1.6;color:#334155;white-space:pre-wrap;word-break:break-all;max-height:none;">${escapeHtml(chunk.content)}</div>
            </div>`;
        });
        html += '</div>';
        
        contentDiv.innerHTML = html;
        modal.style.display = 'block';
    } catch (e) { alert('鍔犺浇棰勮澶辫触: ' + e.message); }
}

async function deletePageIndexDoc(docId, title) {
    if (!confirm(`纭畾鍒犻櫎 PageIndex 鏂囨。 "${title}"锛焋)) return;
    try {
        const res = await fetch(`${API_BASE}/documents/pageindex/delete/${encodeURIComponent(docId)}`, { method: 'POST' });
        const data = await res.json();
        if (data.success) { loadDocuments(); }
        else { alert('鍒犻櫎澶辫触'); }
    } catch (e) { alert('鍒犻櫎澶辫触: ' + e.message); }
}

async function previewPageIndexTree(docId, title) {
    try {
        const res = await fetch(`${API_BASE}/documents/pageindex/tree/${encodeURIComponent(docId)}`);
        const data = await res.json();

        window._pageindexTreeData = data.nodes;

        const modal = document.getElementById('detail-modal');
        const contentDiv = document.getElementById('detail-content');

        let html = `<h3 style="color:#6366f1;margin-bottom:16px;">馃搼 ${escapeHtml(title)} (${data.total} 涓妭鐐?</h3>`;
        html += '<div style="max-height:70vh;overflow-y:auto;background:#fafafa;border-radius:8px;padding:16px;">';

        data.nodes.forEach((node, i) => {
            const level = node.level || 0;
            const indent = level * 20;
            const bgColor = level === 0 ? '#f5f3ff' : (level === 1 ? '#faf5ff' : '#fff');
            const borderColor = level === 0 ? '#e0e7ff' : (level === 1 ? '#e9d5ff' : '#e2e8f0');

            html += `<div style="margin-left:${indent}px;background:${bgColor};border:1px solid ${borderColor};border-radius:6px;padding:10px;margin-bottom:6px;">`;
            html += `<div style="display:flex;align-items:center;gap:6px;margin-bottom:4px;">`;
            if (level === 0) html += '<span style="font-size:16px;">馃摝</span>';
            else if (level === 1) html += '<span style="font-size:14px;">馃搨</span>';
            else html += '<span style="font-size:12px;">馃搫</span>';
            html += `<strong style="color:#3730a3;font-size:${Math.max(13, 16 - level)}px;">${escapeHtml(node.title)}</strong>`;
            html += `<span style="font-size:10px;color:#94a3b8;margin-left:auto;">Lv.${level}</span>`;
            html += '</div>';
            if (node.summary) {
                html += `<div style="font-size:12px;color:#6366f1;font-style:italic;white-space:pre-wrap;padding-left:4px;margin-bottom:4px;">馃摑 ${escapeHtml(node.summary)}</div>`;
            }
            if (node.content) {
                const contentId = `pi-content-${i}`;
                const preview = node.content.length > 150 ? node.content.substring(0, 150) + '...' : node.content;
                html += `<div id="${contentId}" style="font-size:12px;color:#64748b;white-space:pre-wrap;padding-left:4px;">${escapeHtml(preview)}</div>`;
                if (node.content.length > 150) {
                    html += `<button onclick="togglePiContent(${i})" style="font-size:11px;color:#6366f1;background:none;border:none;cursor:pointer;padding-left:4px;">灞曞紑鍏ㄦ枃</button>`;
                }
            }
            html += '</div>';
        });

        html += '</div>';
        contentDiv.innerHTML = html;
        modal.style.display = 'block';
    } catch (e) { alert('鍔犺浇鏍戝け璐? ' + e.message); }
}

function togglePiContent(index) {
    const nodes = window._pageindexTreeData;
    if (!nodes || !nodes[index]) return;
    const el = document.getElementById(`pi-content-${index}`);
    if (!el) return;
    const fullContent = nodes[index].content;
    const preview = fullContent.length > 150 ? fullContent.substring(0, 150) + '...' : fullContent;
    if (el._expanded) {
        el.textContent = preview;
        el._expanded = false;
        const btn = el.nextElementSibling;
        if (btn) btn.textContent = '灞曞紑鍏ㄦ枃';
    } else {
        el.textContent = fullContent;
        el._expanded = true;
        const btn = el.nextElementSibling;
        if (btn) btn.textContent = '鏀惰捣';
    }
}

function expandChunk(el) {
    const content = el.querySelector('.chunk-content');
    if (content._fullText === undefined) {
        // First click: store full text and expand from server later if needed
    }
    content.style.whiteSpace = content.style.whiteSpace === 'pre-wrap' ? 'pre-line' : 'pre-wrap';
    content.style.maxHeight = content.style.maxHeight ? '' : 'none';
}

async function deleteDocument(parentId, filename) {
    if (!confirm(`纭畾鍒犻櫎鏂囨。 "${filename}"锛焅n灏嗗悓鏃跺垹闄?BM25 chunks 鍜屽悜閲忔暟鎹€俙)) return;
    try {
        const res = await fetch(`${API_BASE}/documents/${parentId}`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({filename}) });
        const data = await res.json();
        if (data.success) { alert(data.message); loadDocuments(); fetchStats(); }
        else { alert('鍒犻櫎澶辫触: ' + (data.error || '鏈煡閿欒')); }
    } catch (e) { alert('鍒犻櫎澶辫触: ' + e.message); }
}

const CHUNK_STRATEGY_DESC = {
    recursive: '鎸夋钀解啋琛屸啋鍙モ啋瀛楃閫掑綊鍒囧垎锛宑hunk_size=500锛岄€傚悎澶у鏁板満鏅?,
    large: 'chunk_size=1000锛岄€傚悎闇€瑕侀暱涓婁笅鏂囩悊瑙ｇ殑鏂囨。',
    small: 'chunk_size=200锛岄€傚悎绮惧噯妫€绱㈠満鏅?,
    paragraph: '鎸夋钀藉垎鍓诧紙\\n\\n锛夛紝淇濈暀瀹屾暣娈佃惤缁撴瀯',
    token: '鎸?12 tokens鍒囧垎锛岀簿纭帶鍒朵笂涓嬫枃绐楀彛锛屼富娴丷AG鏍囧噯',
    semantic: '鐢‥mbedding妫€娴嬭瘽棰樿竟鐣屽垏鍒?,
    pageindex: 'LLM瀵艰埅鏂囨。鏍戯紝鏃犻渶Embedding鍜屽悜閲忓簱锛屾寜鏍囬灞傜骇缁勭粐',
};

function updateChunkStrategyDesc() {
    const val = document.getElementById('chunk-strategy').value;
    document.getElementById('chunk-strategy-desc').textContent = CHUNK_STRATEGY_DESC[val] || '';
}

async function uploadFile(file) {
    const allowed = ['.txt', '.pdf', '.md', '.json', '.csv'];
    const ext = file.name.substring(file.name.lastIndexOf('.')).toLowerCase();
    if (!allowed.includes(ext)) { showResult('upload-result', 'error', `涓嶆敮鎸佺殑鏂囦欢绫诲瀷: ${ext}`); return; }
    showResult('upload-result', 'loading', '姝ｅ湪涓婁紶鍜屽鐞?..');
    document.getElementById('upload-progress').classList.remove('hidden');
    const formData = new FormData();
    formData.append('file', file);
    formData.append('chunk_strategy', document.getElementById('chunk-strategy').value);
    try {
        const res = await fetch(`${API_BASE}/upload`, { method: 'POST', body: formData });
        const data = await res.json();
        if (data.success) {
            let extra = '';
            if (data.chunk_strategy === 'pageindex') {
                extra = `<br><a href="#" onclick="showTab('pageindex_search');return false;" style="color:#6366f1;">馃搼 鏌ョ湅鏂囨。鏍?/a>`;
            }
            showResult('upload-result', 'success', `${data.message}<br>鏂囨。鍧楁暟: ${data.chunk_count}${extra}`);
        } else showResult('upload-result', 'error', data.error || '涓婁紶澶辫触');
        fetchStats();
    } catch (e) { showResult('upload-result', 'error', `涓婁紶澶辫触: ${e.message}`); }
    document.getElementById('upload-progress').classList.add('hidden');
}

async function uploadMultipleFiles(files) {
    const allowed = ['.txt', '.pdf', '.md', '.json', '.csv'];
    let successCount = 0;
    let failCount = 0;
    let totalChunks = 0;
    const strategy = document.getElementById('chunk-strategy').value;
    
    showResult('upload-result', 'loading', `姝ｅ湪鎵归噺涓婁紶 ${files.length} 涓枃浠?..`);
    document.getElementById('upload-progress').classList.remove('hidden');
    
    for (let i = 0; i < files.length; i++) {
        const file = files[i];
        const ext = file.name.substring(file.name.lastIndexOf('.')).toLowerCase();
        if (!allowed.includes(ext)) { failCount++; continue; }
        
        const formData = new FormData();
        formData.append('file', file);
        formData.append('chunk_strategy', strategy);
        
        try {
            const res = await fetch(`${API_BASE}/upload`, { method: 'POST', body: formData });
            const data = await res.json();
            if (data.success) { successCount++; totalChunks += data.chunk_count; }
            else failCount++;
            
            const progress = ((i + 1) / files.length) * 100;
            document.getElementById('progress-bar').style.width = progress + '%';
            showResult('upload-result', 'loading', `涓婁紶杩涘害: ${i+1}/${files.length}`);
        } catch (e) { failCount++; }
    }
    
    document.getElementById('upload-progress').classList.add('hidden');
    showResult('upload-result', 'success', `鎵归噺涓婁紶瀹屾垚<br>鎴愬姛: ${successCount} 涓?br>澶辫触: ${failCount} 涓?br>鎬诲潡鏁? ${totalChunks}`);
    fetchStats(); loadDocuments();
}

async function bm25Search() {
    const query = document.getElementById('bm25-query').value.trim();
    const topK = parseInt(document.getElementById('bm25-top-k').value) || 5;
    if (!query) { showResult('bm25-results', 'error', '璇疯緭鍏ユ悳绱㈠唴瀹?); return; }
    saveSearchHistory(query);
    showResult('bm25-results', 'loading', '姝ｅ湪鎼滅储...');
    currentPage = 1;
    try {
        const res = await fetch(`${API_BASE}/search/bm25`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({query, top_k: topK}) });
        const data = await res.json();
        window._bm25Results = data.results;
        document.getElementById('bm25-results').innerHTML = `<h3 style="color: #4caf50; margin-top: 15px;">BM25妫€绱?(${data.total_count}鏉?</h3>${renderSearchHistory()}${renderPaginatedResults('bm25', data.results)}`;
    } catch (e) { showResult('bm25-results', 'error', `鎼滅储澶辫触: ${e.message}`); }
}

async function vectorSearch() {
    const query = document.getElementById('vector-query').value.trim();
    const topK = parseInt(document.getElementById('vector-top-k').value) || 5;
    if (!query) { showResult('vector-results', 'error', '璇疯緭鍏ユ悳绱㈠唴瀹?); return; }
    saveSearchHistory(query);
    showResult('vector-results', 'loading', '姝ｅ湪鎼滅储...');
    currentPage = 1;
    try {
        const res = await fetch(`${API_BASE}/search/vector`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({query, top_k: topK}) });
        const data = await res.json();
        window._vectorResults = data.results;
        document.getElementById('vector-results').innerHTML = `<h3 style="color: #e94560; margin-top: 15px;">鍚戦噺妫€绱?(${data.total_count}鏉?</h3>${renderSearchHistory()}${renderPaginatedResults('vector', data.results)}`;
    } catch (e) { showResult('vector-results', 'error', `鎼滅储澶辫触: ${e.message}`); }
}

async function pageindexSearch() {
    const query = document.getElementById('pageindex-query').value.trim();
    const topK = parseInt(document.getElementById('pageindex-top-k').value) || 10;
    if (!query) { showResult('pageindex-results', 'error', '璇疯緭鍏ユ悳绱㈠唴瀹?); return; }
    showResult('pageindex-results', 'loading', '姝ｅ湪鎼滅储...');
    try {
        const res = await fetch(`${API_BASE}/search/pageindex`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({query, top_k: topK}) });
        const results = await res.json();
        if (results.length === 0) {
            showResult('pageindex-results', 'error', '鏈壘鍒板尮閰嶇粨鏋?);
            return;
        }
        let html = `<h3 style="color: #6366f1; margin-top: 15px;">PageIndex (${results.length}鏉?</h3>`;
        html += '<div style="display:grid;gap:10px;margin-top:10px;">';
        results.forEach((r, i) => {
            html += `<div style="background:#f5f3ff;border:1px solid #e0e7ff;border-radius:8px;padding:12px;">
                <div style="display:flex;gap:8px;align-items:center;margin-bottom:6px;">
                    <span style="background:#6366f1;color:white;padding:2px 8px;border-radius:4px;font-size:11px;">${i+1}</span>
                    <strong style="font-size:14px;color:#3730a3;">${escapeHtml(r.title)}</strong>
                    <span style="font-size:11px;color:#94a3b8;margin-left:auto;">馃搫 ${escapeHtml(r.doc_title)}</span>
                </div>
                <div style="font-size:12px;color:#64748b;margin-bottom:4px;">璺緞: ${escapeHtml(r.path)}</div>
                ${r.summary ? `<div style="font-size:12px;color:#6366f1;font-style:italic;margin-bottom:4px;">馃摑 ${escapeHtml(r.summary)}</div>` : ''}
                <div style="font-size:13px;color:#334155;white-space:pre-wrap;">${escapeHtml(r.content)}</div>
            </div>`;
        });
        html += '</div>';
        document.getElementById('pageindex-results').innerHTML = html;
    } catch (e) { showResult('pageindex-results', 'error', `鎼滅储澶辫触: ${e.message}`); }
}

async function compareSearch() {
    const query = document.getElementById('compare-query').value.trim();
    const topK = parseInt(document.getElementById('compare-top-k').value) || 5;
    if (!query) { showResult('compare-results', 'error', '璇疯緭鍏ユ悳绱㈠唴瀹?); return; }
    showResult('compare-results', 'loading', '姝ｅ湪瀵规瘮涓夌鎼滅储妯″紡...');
    try {
        const [vectorRes, bm25Res, hybridRes] = await Promise.all([
            fetch(`${API_BASE}/search/vector`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({query, top_k: topK}) }),
            fetch(`${API_BASE}/search/bm25`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({query, top_k: topK}) }),
            fetch(`${API_BASE}/search/hybrid`, { method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({query, top_k: topK}) })
        ]);
        const vectorData = await vectorRes.json();
        const bm25Data = await bm25Res.json();
        const hybridData = await hybridRes.json();
        document.getElementById('compare-results').innerHTML = `<div style="display: grid; grid-template-columns: repeat(3, 1fr); gap: 15px; margin-top: 20px;"><div><h3 style="color: #e94560; margin-bottom: 10px;">鍚戦噺妫€绱?(${vectorData.total_count}鏉?</h3>${renderResults(vectorData.results)}</div><div><h3 style="color: #4caf50; margin-bottom: 10px;">BM25妫€绱?(${bm25Data.total_count}鏉?</h3>${renderResults(bm25Data.results)}</div><div><h3 style="color: #c73e54; margin-bottom: 10px;">娣峰悎妫€绱?(${hybridData.total_count}鏉?</h3>${renderResults(hybridData.results)}</div></div>`;
    } catch (e) { showResult('compare-results', 'error', `鎼滅储澶辫触: ${e.message}`); }
}

function renderResults(results) {
    if (results.length === 0) return '<p style="color: #64748b;">鏃犵粨鏋?/p>';
    window._searchResults = results;
    return results.map((r, idx) => {
        const source = r.source || 'unknown';
        const isBM25 = source === 'bm25';
        const scoreDisplay = isBM25 ? r.score.toFixed(2) : (r.score * 100).toFixed(1) + '%';
        const scoreLabel = isBM25 ? 'BM25鍒嗘暟' : '鐩镐技搴?;
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
                <button onclick="copyToClipboard('${fullId}')" style="padding:4px 10px;background:#3b82f6;color:white;border:none;border-radius:4px;font-size:12px;cursor:pointer;">澶嶅埗</button>
            </div>
            
            <div style="color:#1e293b;line-height:1.8;font-size:15px;white-space:pre-wrap;word-break:break-word;margin-bottom:16px;padding:12px;background:#f8fafc;border-radius:8px;">${escapeHtml(shortContent)}</div>
            
            <div style="display:flex;gap:10px;">
                <button onclick="openDetailModal(${idx})" style="padding:10px 20px;background:#1e40af;color:white;border:none;border-radius:8px;font-size:14px;cursor:pointer;font-weight:500;">馃搫 鏌ョ湅瀹屾暣璇︽儏</button>
            </div>
        </div>`;
    }).join('');
}

function copyToClipboard(text) {
    navigator.clipboard.writeText(text).then(() => {
        showToast('宸插鍒跺埌鍓创鏉?);
    }).catch(err => {
        console.error('澶嶅埗澶辫触:', err);
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
    let html = '<div class="search-history"><span style="color:#64748b;font-size:13px;">鎼滅储鍘嗗彶锛?/span>';
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
    if (results.length === 0) return '<p style="color: #64748b;">鏃犵粨鏋?/p>';
    window._searchResults = results;
    
    const totalPages = Math.ceil(results.length / pageSize);
    const start = (currentPage - 1) * pageSize;
    const end = start + pageSize;
    const pageResults = results.slice(start, end);
    
    let html = renderResults(pageResults);
    
    if (totalPages > 1) {
        html += `<div class="pagination">
            <button class="page-btn" onclick="changePage('${type}', -1)" ${currentPage === 1 ? 'disabled' : ''}>涓婁竴椤?/button>
            <span class="page-info">${currentPage}/${totalPages}</span>
            <button class="page-btn" onclick="changePage('${type}', 1)" ${currentPage === totalPages ? 'disabled' : ''}>涓嬩竴椤?/button>
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
        `<h3 style="color: #4caf50; margin-top: 15px;">${type}妫€绱?(${results.length}鏉?</h3>` +
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
                    <div style="font-size:13px;color:#475569;margin-bottom:6px;">鎼滅储鏉ユ簮</div>
                    <div style="font-size:18px;font-weight:700;color:#1e40af;">${source}</div>
                </div>
                <div style="background:#f1f5f9;padding:20px;border-radius:10px;border:1px solid #cbd5e1;">
                    <div style="font-size:13px;color:#475569;margin-bottom:6px;">${isBM25 ? 'BM25鍒嗘暟' : '鐩镐技搴?}</div>
                    <div style="font-size:18px;font-weight:700;color:#059669;">${scoreDisplay}</div>
                </div>
            </div>
            
            <div style="background:#f1f5f9;padding:16px;border-radius:10px;border:1px solid #cbd5e1;">
                <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:8px;">
                    <span style="font-size:13px;color:#1e40af;font-weight:600;">Chunk ID</span>
                    <button onclick="copyToClipboard('${fullId}')" style="padding:6px 12px;background:#3b82f6;color:white;border:none;border-radius:6px;font-size:13px;cursor:pointer;">澶嶅埗</button>
                </div>
                <div style="font-family:monospace;font-size:14px;color:#1e293b;word-break:break-all;background:white;padding:10px;border-radius:6px;line-height:1.5;">${fullId}</div>
            </div>
            
            ${parentId ? `<div style="background:#f1f5f9;padding:16px;border-radius:10px;border:1px solid #cbd5e1;">
                <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:8px;">
                    <span style="font-size:13px;color:#1e40af;font-weight:600;">Parent ID (鏂囨。ID)</span>
                    <button onclick="copyToClipboard('${parentId}')" style="padding:6px 12px;background:#3b82f6;color:white;border:none;border-radius:6px;font-size:13px;cursor:pointer;">澶嶅埗</button>
                </div>
                <div style="font-family:monospace;font-size:14px;color:#1e293b;word-break:break-all;background:white;padding:10px;border-radius:6px;line-height:1.5;">${parentId}</div>
            </div>` : ''}
            
            ${isMerged ? `<div style="background:#fef3c7;padding:16px;border-radius:10px;border:1px solid #fcd34d;">
                <div style="font-size:15px;color:#b45309;font-weight:600;">鈿狅笍 姝ょ粨鏋滅敱澶氫釜chunk鍚堝苟鑰屾垚</div>
            </div>` : ''}
            
            <div style="background:#f1f5f9;padding:16px;border-radius:10px;border:1px solid #cbd5e1;">
                <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:10px;">
                    <span style="font-size:13px;color:#1e40af;font-weight:600;">瀹屾暣鍐呭 (${fullContent.length} 瀛楃)</span>
                    <button onclick="copyText()" style="padding:6px 12px;background:#059669;color:white;border:none;border-radius:6px;font-size:13px;cursor:pointer;">澶嶅埗鍐呭</button>
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
    if (!confirm('纭畾瑕佹竻绌烘墍鏈夋暟鎹悧锛熷寘鎷枃妗ｃ€佺储寮曞拰瀵硅瘽鍘嗗彶锛?)) return;
    try {
        const res = await fetch(`${API_BASE}/clear`, {method: 'POST'});
        const data = await res.json();
        if (data.success) { showResult('upload-result', 'success', data.message); currentSessionId = null; document.getElementById('chat-messages').innerHTML = '<div style="text-align: center; color: #a2a2a2; padding: 40px;">鏁版嵁宸叉竻绌猴紝寮€濮嬫柊瀵硅瘽</div>'; fetchStats(); loadSessions(); }
        else showResult('upload-result', 'error', data.error);
    } catch (e) { showResult('upload-result', 'error', `娓呯┖澶辫触: ${e.message}`); }
}

async function loadSessions() {
    try {
        const res = await fetch(`${API_BASE}/chat/sessions`);
        const sessions = await res.json();
        let html = '<div style="margin-bottom:10px;">';
        html += '<input type="text" id="session-search" placeholder="鎼滅储浼氳瘽..." style="width:100%;padding:8px;border:1px solid #e2e8f0;border-radius:6px;" onkeypress="if(event.key===\'Enter\')searchSessions(this.value)">';
        html += `<button class="btn btn-small" onclick="importSession()" style="margin-top:8px;">馃摜 瀵煎叆</button>`;
        html += '</div>';
        sessions.forEach(s => {
            const isActive = s.session_id === currentSessionId;
            const title = s.title || s.preview || '鏂板璇?;
            const shortId = s.session_id.substring(0, 8) + '...';
            html += `<div class="session-item ${isActive ? 'active' : ''}" onclick="loadSession('${s.session_id}')">
                <div class="session-title">${escapeHtml(title)}</div>
                <div class="time">${formatTime(s.created_at)} (${s.message_count}鏉?</div>
                <div style="font-size:10px;color:#94a3b8;font-family:monospace;margin-top:2px;">ID: ${shortId}</div>
            </div>`;
        });
        document.getElementById('session-list').innerHTML = html || '<div style="color: #a2a2a2; font-size: 12px;">鏆傛棤鍘嗗彶浼氳瘽</div>';
    } catch (e) { document.getElementById('session-list').innerHTML = `<div class="error">鍔犺浇澶辫触: ${e.message}</div>`; }
}

async function loadAllSessions() {
    try {
        const res = await fetch(`${API_BASE}/chat/sessions`);
        const sessions = await res.json();
        let html = '<div style="display: grid; gap: 10px;">';
        sessions.forEach(s => { html += `<div class="session-item" onclick="loadSession('${s.session_id}'); showTab('chat');"><div style="display: flex; justify-content: space-between;"><span>${s.preview || '鏂板璇?}</span><button class="btn btn-small btn-danger" onclick="event.stopPropagation(); deleteSession('${s.session_id}')">鍒犻櫎</button></div><div class="time">${formatTime(s.created_at)} | ${s.message_count}鏉℃秷鎭?/div></div>`; });
        html += '</div>';
        document.getElementById('sessions-list').innerHTML = html || '<div style="color: #a2a2a2;">鏆傛棤浼氳瘽</div>';
    } catch (e) { document.getElementById('sessions-list').innerHTML = `<div class="error">鍔犺浇澶辫触: ${e.message}</div>`; }
}

async function loadSession(sessionId) {
    currentSessionId = sessionId;
    updateSessionDisplay(sessionId);
    try {
        const res = await fetch(`${API_BASE}/chat/history/${sessionId}`);
        const messages = await res.json();
        // 璁＄畻鏈瀵硅瘽鐨勬€?token
        var totalTokens = 0;
        messages.forEach(function(m) { if (m.tokens) totalTokens += m.tokens; });
        let html = '<div style="margin-bottom:10px;display:flex;gap:10px;align-items:center;">';
        html += `<button class="btn btn-small" onclick="exportSession('${sessionId}')">馃摛 瀵煎嚭浼氳瘽</button>`;
        html += `<span style="margin-left:auto;font-size:12px;color:#64748b;">鎬绘秷鑰? <strong style="color:#1e40af;">${totalTokens}</strong> tokens</span>`;
        html += '</div>';
        messages.forEach(m => {
            const roleClass = m.role === 'user' ? 'user' : 'assistant';
            const msgId = m.id;
            const tokenBadge = m.tokens ? `<span style="background:#e2e8f0;color:#64748b;border-radius:4px;padding:1px 6px;font-size:10px;margin-left:8px;">${m.tokens}t</span>` : '';
            html += `<div class="message ${roleClass}" data-msg-id="${msgId}">
                <div class="message-content">${escapeHtml(m.content)}</div>
                <div class="message-actions">
                    <button class="msg-btn copy-btn" onclick="copyMessage('${msgId}')">馃搵</button>
                    <button class="msg-btn edit-btn" onclick="editMessageUI('${msgId}')">鉁忥笍</button>
                    ${m.role === 'assistant' ? `<button class="msg-btn regen-btn" onclick="regenerateMessage('${msgId}')">馃攧</button>` : ''}
                    <button class="msg-btn branch-btn" onclick="branchSession('${sessionId}', '${msgId}')">馃尶</button>
                    <button class="msg-btn delete-btn" onclick="deleteMessageUI('${msgId}')">馃棏锔?/button>
                </div>
                <div class="time">${formatTime(m.time_created)}${tokenBadge}</div>
            </div>`;
        });
        document.getElementById('chat-messages').innerHTML = html || '<div style="text-align: center; color: #a2a2a2; padding: 40px;">绌轰細璇?/div>';
        loadSessions();
    } catch (e) { document.getElementById('chat-messages').innerHTML = `<div class="error">鍔犺浇澶辫触: ${e.message}</div>`; }
}

function updateTotalTokens() {
    if (!currentSessionId) return;
    fetch(`${API_BASE}/chat/history/${currentSessionId}`).then(function(r){return r.json();}).then(function(msgs){
        var total = 0;
        msgs.forEach(function(m){ if(m.tokens) total += m.tokens; });
        document.getElementById('compress-hint').innerHTML = '馃挵 ' + total + ' tokens';
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
        showToast('鉁?Session ID 宸插鍒?);
    } catch (e) {
        prompt('鎵嬪姩澶嶅埗 Session ID:', window._currentSessionId);
    }
    document.body.removeChild(input);
}

async function showContextEditor() {
    const sid = window._currentSessionId;
    if (!sid) { showToast('璇峰厛鍒涘缓鎴栭€夋嫨浼氳瘽'); return; }
    try {
        const res = await fetch(`/api/chat/context/${sid}`);
        const data = await res.json();
        const current = data.context || '';
        const newContext = prompt('缂栬緫閲嶈涓婁笅鏂囷紙LLM浼氳嚜鍔ㄦ彁鍙栵紝浣犱篃鍙互鎵嬪姩淇敼锛夛細', current);
        if (newContext === null) return;
        await fetch(`/api/chat/context/${sid}`, {
            method: 'PUT',
            headers: {'Content-Type': 'application/json'},
            body: JSON.stringify({ context: newContext })
        });
        showToast('閲嶈涓婁笅鏂囧凡鏇存柊');
    } catch (e) {
        showToast('鍔犺浇澶辫触: ' + e.message);
    }
}

function updateSessionDisplay(sid) {
    if (!sid) return;
    document.getElementById('session-id-display').innerHTML = '馃啍 ' + sid;
    document.getElementById('session-copy-btn').style.display = 'inline-block';
    document.getElementById('context-btn').style.display = 'inline-block';
    window._currentSessionId = sid;
    setTimeout(function() { updateTotalTokens(); }, 300);
}

async function newSession() {
    currentSessionId = null;
    localStorage.removeItem('chat_session_id');
    document.getElementById('chat-messages').innerHTML = '<div style="text-align: center; color: #a2a2a2; padding: 40px;"><p>寮€濮嬫柊瀵硅瘽</p></div>';
    document.getElementById('session-id-display').innerHTML = '馃挰 寰呭垱寤?;
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
    assistantDiv.innerHTML = '<div class="streaming">姝ｅ湪鎬濊€?..</div>';
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
                    else if (currentEvent === 'done' || data === '[DONE]') { if (sessionId) { currentSessionId = sessionId; updateSessionDisplay(sessionId); localStorage.setItem('chat_session_id', sessionId); } window.typewriterDone = true;
                        setTimeout(() => { window.typewriterRunning = false; window.typewriterDone = false; var estTokens = Math.ceil(fullReply.length / 4); assistantDiv.innerHTML = `<div>${escapeHtml(fullReply)}</div><div class="time">${new Date().toLocaleTimeString()} <span style="background:#e2e8f0;color:#64748b;border-radius:4px;padding:1px 6px;font-size:10px;margin-left:8px;">~${estTokens}t</span></div>`; if (sourcesCount > 0) assistantDiv.innerHTML += `<div class="sources">鍙傝€冩枃妗? ${sourcesCount}鏉?/div>`; messagesDiv.scrollTop = messagesDiv.scrollHeight; updateTotalTokens(); loadSessions(); fetchStats(); }, 100); currentEvent = ''; }
                    else if (currentEvent === 'error') { assistantDiv.innerHTML = `<div class="error">${data}</div>`; currentEvent = ''; }
                    else if (data && data !== '[DONE]') { fullReply += data; if (!window.typewriterQueue) window.typewriterQueue = ''; window.typewriterQueue += data; if (!window.typewriterRunning) { window.typewriterRunning = true; typeWriterEffect(assistantDiv, messagesDiv); } }
                }
            }
        }
    } catch (e) { assistantDiv.innerHTML = `<div class="error">鍙戦€佸け璐? ${e.message}</div>`; }
    input.disabled = false; document.getElementById('send-btn').disabled = false; input.focus();
}

function typeWriterEffect(assistantDiv, messagesDiv) {
    if (!window.typewriterQueue || window.typewriterQueue.length === 0) { if (window.typewriterDone) return; setTimeout(() => typeWriterEffect(assistantDiv, messagesDiv), 50); return; }
    const charsToShow = Math.min(window.typewriterQueue.length, 2);
    displayedText += window.typewriterQueue.substring(0, charsToShow);
    window.typewriterQueue = window.typewriterQueue.substring(charsToShow);
    assistantDiv.innerHTML = `<div>${escapeHtml(displayedText)}<span class="streaming">鈻?/span></div>`;
    messagesDiv.scrollTop = messagesDiv.scrollHeight;
    const delay = window.typewriterQueue.length > 50 ? 5 : 15;
    setTimeout(() => typeWriterEffect(assistantDiv, messagesDiv), delay);
}

async function deleteSession(sessionId) {
    if (!confirm('纭畾瑕佸垹闄よ繖涓細璇濆悧锛?)) return;
    try {
        await fetch(`${API_BASE}/chat/clear/${sessionId}`, {method: 'POST'});
        if (currentSessionId === sessionId) newSession();
        loadAllSessions(); fetchStats();
    } catch (e) { alert(`鍒犻櫎澶辫触: ${e.message}`); }
}

function copyMessage(msgId) {
    const msgEl = document.querySelector(`.message[data-msg-id="${msgId}"] .message-content`);
    if (msgEl) {
        navigator.clipboard.writeText(msgEl.textContent).then(() => {
            showToast('宸插鍒跺埌鍓创鏉?);
        });
    }
}

async function editMessageUI(msgId) {
    const msgEl = document.querySelector(`.message[data-msg-id="${msgId}"]`);
    const contentEl = msgEl.querySelector('.message-content');
    const originalContent = contentEl.textContent;
    
    const newContent = prompt('缂栬緫娑堟伅鍐呭:', originalContent);
    if (newContent === null || newContent === originalContent) return;
    
    try {
        await fetch(`${API_BASE}/chat/message/${msgId}`, {
            method: 'PUT',
            headers: {'Content-Type': 'application/json'},
            body: JSON.stringify({content: newContent})
        });
        contentEl.textContent = newContent;
        showToast('娑堟伅宸叉洿鏂?);
    } catch (e) {
        alert('鏇存柊澶辫触: ' + e.message);
    }
}

async function deleteMessageUI(msgId) {
    if (!confirm('纭畾鍒犻櫎杩欐潯娑堟伅锛?)) return;
    
    try {
        await fetch(`${API_BASE}/chat/message/${msgId}`, {method: 'DELETE'});
        const msgEl = document.querySelector(`.message[data-msg-id="${msgId}"]`);
        if (msgEl) msgEl.remove();
        showToast('娑堟伅宸插垹闄?);
        loadSessions();
    } catch (e) {
        alert('鍒犻櫎澶辫触: ' + e.message);
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
    showToast(isDark ? '宸插垏鎹㈠埌娣辫壊妯″紡' : '宸插垏鎹㈠埌娴呰壊妯″紡');
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
        document.getElementById('monitor-success-rate').textContent = '鎴愬姛鐜? ' + successRate;
        
        const avgDuration = stats.avg_duration_today_ms > 0 ? stats.avg_duration_today_ms + 'ms' : '--';
        document.getElementById('monitor-avg-duration').textContent = '骞冲潎鑰楁椂: ' + avgDuration;
        
        let typesHtml = '';
        stats.api_types.forEach(t => {
            typesHtml += `<div style="display:flex;justify-content:space-between;padding:12px;background:#f8fafc;border-radius:8px;">
                <span style="font-weight:500;color:#1e40af;">${t.api_type}</span>
                <div style="display:flex;gap:20px;">
                    <span style="color:#64748b;">璋冪敤: ${t.call_count}</span>
                    <span style="color:#059669;">Token: ${t.tokens_used}</span>
                    <span style="color:#f59e0b;">骞冲潎: ${t.avg_duration_ms}ms</span>
                </div>
            </div>`;
        });
        document.getElementById('monitor-api-types').innerHTML = typesHtml || '<div style="color:#64748b;">鏆傛棤鏁版嵁</div>';
        
        renderMonitorChart(stats);
    } catch (e) {
        console.error('鍔犺浇鐩戞帶鏁版嵁澶辫触:', e);
        showToast('鍔犺浇鐩戞帶鏁版嵁澶辫触');
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
                label: '璋冪敤娆℃暟',
                data: stats.api_types.map(t => t.call_count),
                backgroundColor: '#3b82f6'
            }, {
                label: 'Token娑堣€?,
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
    // token鏄剧ず鍦╟ompress-hint涓紝鐢眜pdateTotalTokens鎺у埗
}

function updateCompressUI() {
    const mode = document.getElementById('compress-mode').value;
    const show = mode === 'sliding_window' || mode === 'token_limit' || mode === 'summary' || mode === 'afm' || mode === 'topic';
    document.getElementById('compress-count-wrap').style.display = show ? 'inline' : 'none';
    document.getElementById('compress-count-label').textContent = 
        mode === 'token_limit' ? '闄愬埗' :
        mode === 'summary' ? '瓒呰繃' :
        mode === 'afm' ? '棰勭畻' : '淇濈暀';
    document.getElementById('compress-count-unit').textContent = 
        mode === 'token_limit' ? 'tokens' :
        mode === 'summary' ? '鏉¤Е鍙? :
        mode === 'afm' ? 'tokens' : '鏉?;
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
    const keywords = ['鍚嶅瓧', '璁惧畾', '瑙掕壊'];
    let flowHtml = '';
    
    let importantCount = 0;
    let compressCount = 0;
    
    for (let i = 1; i <= total; i++) {
        const isImportant = keywords.some(k => i % 7 === 0);
        const isRecent = i > total - keepRecent;
        
        if (total > threshold) {
            if (isImportant) {
                flowHtml += `<span class="msg-block msg-important">娑堟伅${i} 猸愰噸瑕?/span>`;
                importantCount++;
            } else if (isRecent) {
                flowHtml += `<span class="msg-block msg-recent">娑堟伅${i} 馃搶鏈€杩?/span>`;
            } else {
                flowHtml += `<span class="msg-block msg-compress">娑堟伅${i}</span>`;
                compressCount++;
            }
        } else {
            flowHtml += `<span class="msg-block msg-recent">娑堟伅${i}</span>`;
        }
    }
    
    if (total > threshold && compressCount > 3) {
        flowHtml += `<span class="msg-block msg-summary">馃摑 鎽樿 (${compressCount}鏉″帇缂?</span>`;
    }
    
    document.getElementById('compress-flow').innerHTML = flowHtml;
    
    let resultHtml = '';
    if (total > threshold) {
        const saved = total - (importantCount + 1 + keepRecent);
        resultHtml = `<div style="background:#10b981;color:white;padding:12px 24px;border-radius:8px;">
            鍘嬬缉鐢熸晥锛佸師 ${total} 鏉?鈫?${importantCount + 1 + keepRecent} 鏉★紝鑺傜渷 ${saved} 鏉℃秷鎭?
        </div>`;
    } else {
        resultHtml = `<div style="background:#64748b;color:white;padding:12px 24px;border-radius:8px;">
            娑堟伅鏁?${total} 鏈秴杩囬槇鍊?${threshold}锛屾殏涓嶅帇缂?
        </div>`;
    }
    document.getElementById('compress-result').innerHTML = resultHtml;
}

function showModeDetail(mode) {
    document.querySelectorAll('.mode-card').forEach(c => c.classList.remove('active'));
    event.target.closest('.mode-card').classList.add('active');
    
    const details = {
        'none': `<h4 style="color:#1e40af;">涓嶅帇缂╂ā寮?/h4>
            <p style="color:#475569;line-height:1.8;">淇濈暀鎵€鏈夊巻鍙叉秷鎭紝鐩存帴鍙戦€佺粰LLM銆?/p>
            <p style="color:#dc2626;">鈿狅笍 缂虹偣锛氬彲鑳借秴鍑簍oken闄愬埗锛屽鑷碅PI鎶ラ敊鎴栨埅鏂€?/p>
            <p style="color:#059669;">鉁?閫傜敤锛氱煭瀵硅瘽銆佹祴璇曞満鏅€?/p>`,
        'sliding_window': `<h4 style="color:#1e40af;">婊戝姩绐楀彛妯″紡</h4>
            <p style="color:#475569;line-height:1.8;">鍙繚鐣欐渶杩慛鏉℃秷鎭紝涓㈠純鏇存棭鐨勫巻鍙层€?/p>
            <p style="color:#dc2626;">鈿狅笍 缂虹偣锛氫涪澶遍噸瑕佽瀹氫俊鎭€?/p>
            <p style="color:#059669;">鉁?閫傜敤锛氫笉闇€瑕佽浣忓巻鍙蹭俊鎭殑鍦烘櫙銆?/p>`,
        'token_limit': `<h4 style="color:#1e40af;">Token闄愬埗妯″紡</h4>
            <p style="color:#475569;line-height:1.8;">淇濈暀鍓?鏉℃秷鎭紙鍏抽敭璁惧畾锛夛紝鐒跺悗浠庡熬閮ㄥ線鍓嶄繚鐣欑洿鍒拌揪鍒皌oken涓婇檺銆?/p>
            <p style="color:#dc2626;">鈿狅笍 缂虹偣锛氬彲鑳藉垹闄や腑闂存秷鎭€?/p>
            <p style="color:#059669;">鉁?閫傜敤锛氶渶瑕佹帶鍒禔PI鎴愭湰鐨勫満鏅€傚彲鑷畾涔塼oken涓婇檺銆?/p>`,
        'summary': `<h4 style="color:#1e40af;">鎽樿鍘嬬缉妯″紡</h4>
            <p style="color:#475569;line-height:1.8;">灏嗘墍鏈夊巻鍙叉秷鎭帇缂╂垚涓€鏉℃憳瑕併€?/p>
            <p style="color:#dc2626;">鈿狅笍 缂虹偣锛氭憳瑕佸彲鑳戒涪澶辩粏鑺備俊鎭€?/p>
            <p style="color:#059669;">鉁?閫傜敤锛氶暱瀵硅瘽銆佸彧闇€淇濈暀澶ф剰銆?/p>`,
        'layered': `<h4 style="color:#1e40af;">鍒嗗眰鍘嬬缉妯″紡锛堟帹鑽愶級</h4>
            <p style="color:#475569;line-height:1.8;">
                1锔忊儯 妫€鏌ユ秷鎭暟閲忔槸鍚﹁秴杩囬槇鍊?br>
                2锔忊儯 鎻愬彇鍖呭惈鍏抽敭璇嶇殑娑堟伅 鈫?<span style="color:#059669;font-weight:bold">閲嶈娑堟伅</span><br>
                3锔忊儯 鎻愬彇鏈€杩慛鏉℃秷鎭?鈫?<span style="color:#3b82f6;font-weight:bold">鏈€杩戞秷鎭?/span><br>
                4锔忊儯 鍓╀綑娑堟伅璋冪敤LLM鐢熸垚鎽樿 鈫?<span style="color:#f59e0b;font-weight:bold">鎽樿</span><br>
                5锔忊儯 缁勫悎: 閲嶈 + 鎽樿 + 鏈€杩?鈫?鍙戦€佺粰LLM
            </p>
            <p style="color:#059669;">鉁?閫傜敤锛氱敓浜х幆澧冿紝淇濇姢閲嶈淇℃伅鍚屾椂鑺傜渷token銆?/p>`,
        'afm': `<h4 style="color:#1e40af;">AFM鑷€傚簲淇濈湡搴﹀帇缂?/h4>
            <p style="color:#475569;line-height:1.8;">
                馃敼 LLM 瀵规瘡鏉℃秷鎭垎涓夋。锛?br>
                <span style="color:#059669;font-weight:bold">Full锛堝畬鏁翠繚鐣欙級</span> 鈥?鍏抽敭璁惧畾銆佺敤鎴风害鏉?br>
                <span style="color:#f59e0b;font-weight:bold">Compressed锛堢簿绠€锛?/span> 鈥?鏈夊弬鑰冧环鍊肩殑娑堟伅锛屽帇缂╀负涓€鍙ヨ瘽<br>
                <span style="color:#94a3b8;font-weight:bold">Placeholder锛堝崰浣嶏級</span> 鈥?闂茶亰鎴栨棤鍏冲唴瀹癸紝鏄剧ず"鐪佺暐X鏉?
            </p>
            <p style="color:#059669;">鉁?姣斿垎灞傚帇缂╂洿绮剧粏锛屼俊鎭繚鐣欐洿鍑嗙‘銆?/p>`,
        'topic': `<h4 style="color:#1e40af;">璇濋鍒嗘鍘嬬缉</h4>
            <p style="color:#475569;line-height:1.8;">
                馃敼 LLM 妫€娴嬭瘽棰樺垏鎹㈢偣<br>
                馃敼 姣忎竴娈电嫭绔嬬敓鎴愭憳瑕?br>
                馃敼 淇濈暀鏈€杩戝璇濆畬鏁?br>
                馃敼 绀轰緥锛?br>
                <span style="color:#94a3b8;">[璇濋] "鐢ㄦ埛璇㈤棶Rust鐨勫畾涔夊拰鐢ㄩ€?</span><br>
                <span style="color:#94a3b8;">[璇濋] "鐢ㄦ埛璇锋眰AI鎵紨鐢靛瓙灏忕嫍锛屽彇鍚嶅皬鐖卞悓瀛?</span><br>
                <span style="color:#94a3b8;">[璇濋] "璁ㄨ鑻规灉鍜岃タ鐡滅殑钀ュ吇浠峰€?</span><br>
                <span style="color:#1e293b;">鏈€杩戞秷鎭?..</span>
            </p>
            <p style="color:#059669;">鉁?闀垮璇濅腑鎸夎瘽棰樼粍缁囷紝姣斿崟涓€鎽樿鏇存竻鏅般€?/p>`
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
            document.getElementById('langgraph-action-bar').querySelector('span').textContent = '鐐瑰嚮涓婃柟鎸夐挳鏌ョ湅鍥剧粨鏋?;
        }
    } catch (e) { /* ignore */ }
}

let _decomposedData = null;

async function runDecompose() {
    const task = document.getElementById('decompose-input').value.trim();
    if (!task) { showToast('璇疯緭鍏ヤ换鍔?); return; }

    const viz = document.getElementById('langgraph-viz');
    const container = document.getElementById('mermaid-container');
    const results = document.getElementById('langgraph-results');

    viz.style.display = 'block';
    document.getElementById('graph-title').textContent = '馃 AI 姝ｅ湪鎷嗚В浠诲姟...';
    document.getElementById('graph-desc').textContent = '';
    container.innerHTML = '<div style="text-align:center;color:#94a3b8;padding:30px;">鈴?LLM 鍒嗘瀽涓?..</div>';
    results.innerHTML = '';

    try {
        const res = await fetch(`${API_BASE}/langgraph/decompose`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ task })
        });
        if (!res.ok) {
            const errData = await res.json().catch(() => ({}));
            throw new Error(errData.error || `鏈嶅姟鍣ㄨ繑鍥?${res.status}`);
        }
        const data = await res.json();

        if (!data.sub_tasks || !data.graph_structure) {
            throw new Error('LLM 杩斿洖鏍煎紡寮傚父锛岃閲嶈瘯');
        }

        _decomposedData = data;
        document.getElementById('graph-title').textContent = `馃 浠诲姟鎷嗚В: ${escapeHtml(data.original_task)}`;
        document.getElementById('graph-desc').textContent = `${data.sub_tasks.length} 涓瓙浠诲姟`;
        container.innerHTML = renderGraphHtml(data.graph_structure, {});

        let detailHtml = `<div style="display:flex;justify-content:space-between;align-items:center;margin-top:20px;">`;
        detailHtml += `<h3 style="color:#10b981;margin:0;">馃搵 瀛愪换鍔″垪琛?/h3>`;
        detailHtml += `<button class="btn" onclick="executeDecomposed()" style="background:#10b981;color:white;">鈻?鎵ц鎵€鏈夊瓙浠诲姟</button>`;
        detailHtml += `</div>`;
        detailHtml += `<div style="border:1px solid #e2e8f0;border-radius:8px;padding:15px;margin-top:10px;">
            <table style="width:100%;border-collapse:collapse;">
            <thead><tr style="background:#f1f5f9;">
                <th style="padding:8px;border:1px solid #e2e8f0;">瀛愪换鍔?/th>
                <th style="padding:8px;border:1px solid #e2e8f0;">鎻忚堪</th>
                <th style="padding:8px;border:1px solid #e2e8f0;">渚濊禆</th>
            </tr></thead>
            <tbody>`;
        data.sub_tasks.forEach(st => {
            const deps = (st.depends_on && st.depends_on.length) ? st.depends_on.join(', ') : '鏃?;
            detailHtml += `<tr>
                <td style="padding:8px;border:1px solid #e2e8f0;">${escapeHtml(st.name)}</td>
                <td style="padding:8px;border:1px solid #e2e8f0;">${escapeHtml(st.description)}</td>
                <td style="padding:8px;border:1px solid #e2e8f0;">${deps}</td>
            </tr>`;
        });
        detailHtml += `</tbody></table></div>`;
        results.innerHTML = detailHtml;
    } catch (e) {
        container.innerHTML = `<div style="color:#e94560;text-align:center;padding:20px;">澶辫触: ${e.message}</div>`;
    }
}

async function executeDecomposed() {
    if (!_decomposedData) return;
    const results = document.getElementById('langgraph-results');
    results.innerHTML = '<div style="text-align:center;color:#94a3b8;padding:20px;">鈴?鎵ц涓?..</div>';

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
            throw new Error(errData.error || `鏈嶅姟鍣ㄨ繑鍥?${res.status}`);
        }
        const data = await res.json();

        // 鏇存柊鍥句笂鐨勬敞瑙?
        const annotations = {};
        (data.execution_results || []).forEach(r => {
            annotations[r.name] = { label: r.output, ms: r.duration_ms };
        });
        const container = document.getElementById('mermaid-container');
        container.innerHTML = renderGraphHtml(_decomposedData.graph_structure, annotations);

        // 鏄剧ず缁撴灉琛ㄦ牸锛堝惈 token锛?
        let html = `<h3 style="color:#10b981;margin-top:20px;">鉁?鎵ц瀹屾垚</h3>`;
        html += `<div style="border:1px solid #e2e8f0;border-radius:8px;padding:15px;">
            <table style="width:100%;border-collapse:collapse;">
            <thead><tr style="background:#f1f5f9;">
                <th style="padding:8px;border:1px solid #e2e8f0;">瀛愪换鍔?/th>
                <th style="padding:8px;border:1px solid #e2e8f0;">杈撳嚭</th>
                <th style="padding:8px;border:1px solid #e2e8f0;">鑰楁椂</th>
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

        // 姹囨€?
        const totalTokens = (data.execution_results || []).reduce((s, r) => s + (r.tokens || 0), 0);
        const totalMs = (data.execution_results || []).reduce((s, r) => s + r.duration_ms, 0);
        html += `<p style="margin-top:10px;color:#64748b;font-size:13px;">鎬昏: ${totalMs}ms | ${totalTokens} tokens</p>`;

        results.innerHTML = html;
    } catch (e) {
        results.innerHTML = `<div style="color:#e94560;text-align:center;padding:20px;">鎵ц澶辫触: ${e.message}</div>`;
    }
}

const LG_MODE = {
    parallel: { name: '骞惰鎵ц', color: '#667eea', desc: 'FanOut 鈫?3涓换鍔″悓鏃惰窇' },
    conditional: { name: '鏉′欢璺敱', color: '#f5576c', desc: '鏍规嵁杈撳叆闀垮害鍔ㄦ€侀€夎矾寰? },
    stream: { name: '娴佸紡鎵ц', color: '#4facfe', desc: 'step1鈫抯tep2鈫抯tep3 閫愭鎵ц' },
    subgraph: { name: '瀛愬浘', color: '#059669', desc: '瀛愬浘宓屽锛氱埗鍥剧敓鎴愨啋瀛愬浘瀹℃牳' },
    llm_conditional: { name: 'LLM 璺敱', color: '#d97706', desc: 'LLM 鍒ゆ柇鎰忓浘锛屽姩鎬佽矾鐢? },
};

async function showLangGraphStructure(mode) {
    const input = document.getElementById('langgraph-input').value || '娴嬭瘯杈撳叆';
    const container = document.getElementById('mermaid-container');
    const viz = document.getElementById('langgraph-viz');
    const info = LG_MODE[mode];

    document.getElementById('graph-title').textContent = `馃搻 ${info.name} - 鍥剧粨鏋刞;
    document.getElementById('graph-desc').textContent = info.desc;
    container.innerHTML = '<div style="text-align:center;color:#94a3b8;padding:20px;">鎵ц涓?..</div>';
    viz.style.display = 'block';

    try {
        let execData;
        let strucData;

        if (mode === 'subgraph') {
            execData = await fetch(`${API_BASE}/langgraph/subgraph`, {
                method: 'POST', headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ input })
            }).then(r => r.json());
            // 瀛愬浘鐢ㄧ‖缂栫爜鍥剧粨鏋勶紙妗嗘灦涓嶆敮鎸佸姩鎬佽幏鍙栧瓙鍥?mermaid锛?
            strucData = {
                structure: {
                    entry_point: '鐢熸垚鍐呭',
                    nodes: ['鐢熸垚鍐呭', '璐ㄩ噺瀹℃牳(瀛愬浘)'],
                    edges: [
                        { type: 'fixed', source: '__start__', target: '鐢熸垚鍐呭' },
                        { type: 'fixed', source: '鐢熸垚鍐呭', target: '璐ㄩ噺瀹℃牳(瀛愬浘)' },
                        { type: 'fixed', source: '璐ㄩ噺瀹℃牳(瀛愬浘)', target: '__end__' },
                    ],
                    routers: []
                }
            };
        } else if (mode === 'llm_conditional') {
            execData = await fetch(`${API_BASE}/langgraph/llm_conditional`, {
                method: 'POST', headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ input })
            }).then(r => r.json());
            strucData = {
                structure: {
                    entry_point: '鍒嗘瀽鎰忓浘',
                    nodes: ['鍒嗘瀽鎰忓浘', '鎶€鏈洖绛?, '閫氱敤鍥炵瓟', '鍏滃簳鍥炵瓟'],
                    edges: [
                        { type: 'fixed', source: '__start__', target: '鍒嗘瀽鎰忓浘' },
                        { type: 'conditional', source: '鍒嗘瀽鎰忓浘', router: 'llm_intent_router',
                          targets: { tech: '鎶€鏈洖绛?, general: '閫氱敤鍥炵瓟', other: '鍏滃簳鍥炵瓟' }, default: '鍏滃簳鍥炵瓟' },
                        { type: 'fixed', source: '鎶€鏈洖绛?, target: '__end__' },
                        { type: 'fixed', source: '閫氱敤鍥炵瓟', target: '__end__' },
                        { type: 'fixed', source: '鍏滃簳鍥炵瓟', target: '__end__' },
                    ],
                    routers: ['llm_intent_router']
                }
            };
        } else {
            const [strucRes, exec] = await Promise.all([
                fetch(`${API_BASE}/langgraph/structure`, {
                    method: 'POST', headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ mode })
                }),
                fetch(`${API_BASE}/langgraph/${mode}`, {
                    method: 'POST', headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ input })
                }),
            ]);
            strucData = await strucRes.json();
            execData = await exec.json();
        }

        const annotations = buildAnnotations(mode, execData);
        container.innerHTML = renderGraphHtml(strucData.structure, annotations);
        document.getElementById('langgraph-results').innerHTML = renderExecResults(mode, execData);
    } catch (e) {
        container.innerHTML = `<div style="color:#e94560;text-align:center;padding:20px;">澶辫触: ${e.message}</div>`;
    }
}

function buildAnnotations(mode, data) {
    const ann = {};
    if (mode === 'parallel') {
        const taskMap = { 'TaskA': 'task_a', 'TaskB': 'task_b', 'TaskC': 'task_c' };
        (data.parallel_tasks || []).forEach(t => {
            const nodeName = taskMap[t.task_name] || t.task_name.toLowerCase();
            ann[nodeName] = { label: t.result.substring(0,30), ms: t.duration_ms };
        });
        ann['dispatcher'] = { label: '灏嗕换鍔″垎鍙戠粰 3 涓苟琛岃妭鐐?, ms: 0 };
    } else if (mode === 'conditional') {
        ann['analyze'] = { label: `鍒嗘瀽杈撳叆锛岄暱搴?${data.input.length}`, ms: 0 };
        const nodeName = data.path_taken;
        ann[nodeName] = { label: data.output, ms: 0 };
    } else if (mode === 'stream') {
        (data || []).forEach(e => {
            if (e.event_type === 'complete' || e.event_type === 'enter') {
                ann[e.node_name] = { label: `鎵ц涓璥, ms: e.timestamp_ms };
            }
        });
    } else if (mode === 'subgraph') {
        ann['鐢熸垚鍐呭'] = { label: data.generated_content.substring(0,30), ms: 0 };
        ann['璐ㄩ噺瀹℃牳(瀛愬浘)'] = { label: data.review_result.substring(0,30), ms: 0 };
    } else if (mode === 'llm_conditional') {
        ann['鍒嗘瀽鎰忓浘'] = { label: `杈撳叆: ${data.input}`, ms: 0 };
        if (data.route_taken) {
            ann[data.route_taken] = { label: data.output.substring(0,30), ms: 0 };
        }
    }
    return ann;
}

function renderExecResults(mode, data) {
    const info = LG_MODE[mode];
    let html = `<h3 style="color:${info.color};margin-top:20px;">${info.name}缁撴灉</h3>`;

    if (mode === 'parallel') {
        html += `<div style="border:1px solid #e2e8f0;padding:15px;border-radius:8px;">
            <p><strong>杈撳叆锛?/strong>${escapeHtml(data.input)}</p>
            <p><strong>缁撴灉锛?/strong>${escapeHtml(data.merged_result)}</p>
            <p><strong>鎬昏€楁椂锛?/strong>${data.total_time_ms}ms | <strong>鑺傜渷锛?/strong>${data.time_saved_percent.toFixed(1)}%</p>
            <h4 style="margin-top:10px;">骞惰浠诲姟锛?/h4>
            <ul>${data.parallel_tasks.map(t => `<li>${t.task_name}: ${t.result} (${t.duration_ms}ms)</li>`).join('')}</ul>
        </div>`;
    } else if (mode === 'conditional') {
        html += `<div style="border:1px solid #e2e8f0;padding:15px;border-radius:8px;">
            <p><strong>杈撳叆锛?/strong>${escapeHtml(data.input)} (闀垮害: ${data.input.length})</p>
            <p><strong>璺敱鍐崇瓥锛?/strong>${escapeHtml(data.route_decision)}</p>
            <p><strong>鎵ц璺緞锛?/strong>${escapeHtml(data.path_taken)}</p>
            <p><strong>杈撳嚭锛?/strong>${escapeHtml(data.output)}</p>
            <h4 style="margin-top:10px;">姝ラ锛?/h4>
            <ol>${data.steps.map(s => `<li>${escapeHtml(s)}</li>`).join('')}</ol>
        </div>`;
    } else if (mode === 'stream') {
        html += `<div style="border:1px solid #e2e8f0;padding:15px;border-radius:8px;">
            <p><strong>浜嬩欢鏁帮細</strong>${data.length}</p>
            <table style="width:100%;margin-top:10px;border-collapse:collapse;">
                <thead><tr style="background:#f1f5f9;">
                    <th style="padding:8px;border:1px solid #e2e8f0;">鑺傜偣</th>
                    <th style="padding:8px;border:1px solid #e2e8f0;">浜嬩欢</th>
                    <th style="padding:8px;border:1px solid #e2e8f0;">鏃堕棿(ms)</th>
                </tr></thead>
                <tbody>${data.map(e => `<tr>
                    <td style="padding:8px;border:1px solid #e2e8f0;">${escapeHtml(e.node_name)}</td>
                    <td style="padding:8px;border:1px solid #e2e8f0;">${escapeHtml(e.event_type)}</td>
                    <td style="padding:8px;border:1px solid #e2e8f0;">${e.timestamp_ms}</td>
                </tr>`).join('')}</tbody>
            </table>
        </div>`;
    } else if (mode === 'subgraph') {
        html += `<div style="border:1px solid #e2e8f0;padding:15px;border-radius:8px;">
            <p><strong>杈撳叆锛?/strong>${escapeHtml(data.input)}</p>
            <p><strong>鐢熸垚鍐呭锛?/strong>${escapeHtml(data.generated_content)}</p>
            <p><strong>瀹℃牳缁撴灉锛?/strong>${escapeHtml(data.review_result)}</p>
            <p><strong>鎬昏€楁椂锛?/strong>${data.total_duration_ms}ms</p>
            <div style="background:#f1f5f9;padding:10px;border-radius:6px;font-size:12px;margin-top:10px;">
                <strong>馃挕 鍏抽敭鐐癸細</strong>瀛愬浘鏈夎嚜宸辩殑鐘舵€佺被鍨嬶紙ReviewState锛夛紝
                閫氳繃 input_mapper/output_mapper 涓庣埗鍥撅紙AgentState锛変簰鐩歌浆鎹€?
                鐖跺浘鍜屽瓙鍥惧彲浠ュ悇鑷淮鎶ょ嫭绔嬬殑鐘舵€併€?
            </div>
        </div>`;
    } else if (mode === 'llm_conditional') {
        html += `<div style="border:1px solid #e2e8f0;padding:15px;border-radius:8px;">
            <p><strong>杈撳叆锛?/strong>${escapeHtml(data.input)}</p>
            <p><strong>LLM 鍒ゆ柇璺敱锛?/strong>${escapeHtml(data.route_taken)}</p>
            <p><strong>杈撳嚭锛?/strong>${escapeHtml(data.output)}</p>
            <p><strong>鎬昏€楁椂锛?/strong>${data.total_duration_ms}ms</p>
            <div style="background:#f1f5f9;padding:10px;border-radius:6px;font-size:12px;margin-top:10px;">
                <strong>馃挕 鍏抽敭鐐癸細</strong>涓嶆槸鐢ㄥ浐瀹氳鍒欏垽鏂矾鐢憋紝
                鑰屾槸璋?LLM 鍒嗘瀽闂鎰忓浘锛屾牴鎹?LLM 杩斿洖鍔ㄦ€侀€夋嫨璺緞銆?
                杩欐槸"鐪?Agent"鍜?鍥哄畾娴佹按绾?鐨勫垎姘村箔銆?
            </div>
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
        else {
            // 检查是否是决策节点
            const taskDef = _agentPlanData && _agentPlanData.tasks ? _agentPlanData.tasks.find(t => t.name === n) : null;
            if (taskDef && taskDef.task_type === 'decision') {
                nodeColors[n] = '#f59e0b'; // 橙色=决策节点
            } else {
                nodeColors[n] = '#3b82f6';
            }
        }
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

function renderDecisionNode(name, color, label, annotation, routes) {
    // 决策节点：显示为菱形，标注路由选项
    const bg = color + '22';
    const border = color;
    const routeHtml = routes ? Object.entries(routes).map(([k, v]) =>
        `<div style="font-size:10px;color:#d97706;">${k} → ${v}</div>`
    ).join('') : '';
    return `<div style="display:flex;flex-direction:column;align-items:center;">
        <div style="background:${bg};border:2px solid ${border};border-radius:4px;transform:rotate(45deg);
            width:60px;height:60px;display:flex;align-items:center;justify-content:center;margin:8px;">
            <div style="transform:rotate(-45deg);text-align:center;font-size:11px;font-weight:bold;color:${border};">🤔 ${name}</div>
        </div>
        ${annotation ? `<div style="font-size:10px;color:#64748b;margin-top:2px;max-width:120px;text-align:center;word-break:break-all;">${annotation.label}</div>` : ''}
        ${routeHtml}
    </div>`;
}

function renderFlowLevel(nodes, edges, entry, nodeColors, annotations) {
    // 鐢ㄤ緷璧栧叧绯昏绠楀眰绾э細姣忎釜鑺傜偣鐨刲evel = 1 + max(level of all depends_on)
    const edgesJson = JSON.stringify(edges);
    const depMap = {};  // task name -> level
    nodes.forEach(n => {
        // 鎵捐繖涓妭鐐圭殑渚濊禆
        const deps = edges.filter(e =>
            e.type === 'fixed' && e.target === n && e.source !== '__start__' && e.target !== '__end__'
        ).map(e => e.source);
        // 鎵惧畠鍦ㄤ换鍔″垪琛ㄤ腑鐨?depends_on
        const taskDef = _agentPlanData && _agentPlanData.tasks ? _agentPlanData.tasks.find(t => t.name === n) : null;
        const realDeps = taskDef ? (taskDef.depends_on || []) : deps;
        if (!realDeps || realDeps.length === 0) {
            depMap[n] = 0;
        } else {
            depMap[n] = 1 + Math.max(0, ...realDeps.map(d => (depMap[d] !== undefined ? depMap[d] : 0)));
        }
    });

    const maxLevel = Math.max(0, ...Object.values(depMap));
    const levels = [];
    for (let i = 0; i <= maxLevel; i++) {
        levels.push(Object.keys(depMap).filter(k => depMap[k] === i));
    }

    let html = '';
    levels.forEach((level, li) => {
        if (li > 0) html += renderArrow();
        if (level.length === 1) {
            const n = level[0];
            const taskDef = _agentPlanData && _agentPlanData.tasks ? _agentPlanData.tasks.find(t => t.name === n) : null;
            if (taskDef && taskDef.task_type === 'decision') {
                html += renderDecisionNode(n, nodeColors[n], n, annotations[n] || null, taskDef.routes);
            } else {
                html += renderGraphNode(n, nodeColors[n] || '#3b82f6', n, annotations[n] || null);
            }
        } else {
            html += '<div style="display:flex;gap:24px;justify-content:center;">';
            level.forEach(n => {
                const taskDef = _agentPlanData && _agentPlanData.tasks ? _agentPlanData.tasks.find(t => t.name === n) : null;
                if (taskDef && taskDef.task_type === 'decision') {
                    html += renderDecisionNode(n, nodeColors[n], n, annotations[n] || null, taskDef.routes);
                } else {
                    html += renderGraphNode(n, nodeColors[n] || '#3b82f6', n, annotations[n] || null);
                }
            });
            html += '</div>';
        }
    });
    return html;
}

const NODE_ROLES = {
    dispatcher: { icon: '馃摠', role: '浠诲姟鍒嗗彂' },
    task_a: { icon: '馃摜', role: '鏁版嵁鑾峰彇' },
    task_b: { icon: '馃搫', role: '鏂囨。澶勭悊' },
    task_c: { icon: '馃搳', role: '鍐呭鍒嗘瀽' },
    analyze: { icon: '馃攳', role: '杈撳叆鍒嗘瀽' },
    quick_process: { icon: '鈿?, role: '蹇€熷鐞? },
    detailed_process: { icon: '馃搵', role: '璇︾粏鍒嗘瀽' },
    step1: { icon: '鈶?, role: '绗竴姝? },
    step2: { icon: '鈶?, role: '绗簩姝? },
    step3: { icon: '鈶?, role: '绗笁姝? },
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
    return '<div style="color:#94a3b8;font-size:18px;">鈫?/div>';
}



async function regenerateMessage(msgId) {
    if (!confirm('纭畾閲嶆柊鐢熸垚杩欐潯AI鍥炲锛?)) return;
    try {
        showToast('姝ｅ湪閲嶆柊鐢熸垚...');
        const res = await fetch(`${API_BASE}/chat/message/${msgId}/regenerate`, {method: 'POST'});
        const data = await res.json();
        if (data.message_id) {
            const msgEl = document.querySelector(`.message[data-msg-id="${msgId}"]`);
            if (msgEl) {
                msgEl.querySelector('.message-content').textContent = data.reply;
                msgEl.dataset.msgId = data.message_id;
            }
            showToast('宸查噸鏂扮敓鎴?);
            loadSessions();
        } else {
            showToast('閲嶆柊鐢熸垚澶辫触');
        }
    } catch (e) { showToast('閲嶆柊鐢熸垚澶辫触: ' + e.message); }
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
        showToast('浼氳瘽宸插鍑?);
    } catch (e) { showToast('瀵煎嚭澶辫触: ' + e.message); }
}

async function branchSession(sessionId, fromMsgId) {
    if (!confirm('纭畾浠庢娑堟伅鍒涘缓鍒嗘敮锛?)) return;
    try {
        const res = await fetch(`${API_BASE}/chat/session/branch`, {
            method: 'POST',
            headers: {'Content-Type': 'application/json'},
            body: JSON.stringify({session_id: sessionId, from_message_id: fromMsgId})
        });
        const data = await res.json();
        if (data.new_session_id) {
            showToast(`宸插垱寤哄垎鏀? ${data.title}`);
            currentSessionId = data.new_session_id;
            loadSession(data.new_session_id);
            loadSessions();
        }
    } catch (e) { showToast('鍒涘缓鍒嗘敮澶辫触: ' + e.message); }
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
                showToast('浼氳瘽瀵煎叆鎴愬姛');
                loadSessions(); loadAllSessions();
            } else {
                showToast('瀵煎叆澶辫触');
            }
        } catch (e) { showToast('瀵煎叆澶辫触: ' + e.message); }
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
                <div class="time">${formatTime(s.created_at)} | ${s.message_count}鏉?/div>
            </div>`;
        });
        html += '</div>';
        document.getElementById('sessions-list').innerHTML = html || '<div style="color:#64748b;">鏃犲尮閰嶄細璇?/div>';
    } catch (e) { showToast('鎼滅储澶辫触: ' + e.message); }
}

async function batchDeleteDocuments() {
    const checkboxes = document.querySelectorAll('.doc-checkbox:checked');
    const ids = Array.from(checkboxes).map(cb => cb.value);
    if (ids.length === 0) { showToast('璇烽€夋嫨瑕佸垹闄ょ殑鏂囨。'); return; }
    if (!confirm(`纭畾鍒犻櫎 ${ids.length} 涓枃妗ｏ紵`)) return;
    try {
        const res = await fetch(`${API_BASE}/documents/batch-delete`, {
            method: 'POST',
            headers: {'Content-Type': 'application/json'},
            body: JSON.stringify({parent_ids: ids})
        });
        const data = await res.json();
        showToast(data.message);
        loadDocuments(); fetchStats();
    } catch (e) { showToast('鍒犻櫎澶辫触: ' + e.message); }
}

async function addDocumentTags(parentId) {
    const tags = prompt('杈撳叆鏍囩锛堥€楀彿鍒嗛殧锛?');
    if (!tags) return;
    const tagList = tags.split(',').map(t => t.trim()).filter(t => t);
    try {
        await fetch(`${API_BASE}/documents/tags`, {
            method: 'POST',
            headers: {'Content-Type': 'application/json'},
            body: JSON.stringify({parent_id: parentId, tags: tagList})
        });
        showToast('鏍囩宸叉坊鍔?);
        loadDocuments();
    } catch (e) { showToast('娣诲姞鏍囩澶辫触: ' + e.message); }
}

async function loadDocumentsByTag(tag) {
    try {
        const res = await fetch(`${API_BASE}/documents/tag/${encodeURIComponent(tag)}`);
        const documents = await res.json();
        let html = `<h3 style="color:#1e40af;margin-bottom:15px;">鏍囩: ${escapeHtml(tag)} (${documents.length}涓枃妗?</h3>`;
        html += '<table style="width:100%;border-collapse:collapse;"><tbody>';
        documents.forEach(doc => {
            html += `<tr><td style="padding:12px;border:1px solid #e2e8f0;">${escapeHtml(doc.title)}</td>
                <td style="padding:12px;border:1px solid #e2e8f0;text-align:center;">${doc.chunk_count}</td>
                <td style="padding:12px;border:1px solid #e2e8f0;"><button class="btn btn-small" onclick="previewDocument('${doc.id}')">棰勮</button></td></tr>`;
        });
        html += '</tbody></table>';
        document.getElementById('documents-list').innerHTML = html || '<div style="color:#64748b;">鏃犳枃妗?/div>';
    } catch (e) { showToast('鍔犺浇澶辫触: ' + e.message); }
}

let _agentPlanData = null;

// 鈽?鎵ц鎵€鏈変换鍔★紙鐪熸骞惰锛?
async function runAgentExecute() {
    if (!_agentPlanData) { showToast('璇峰厛瑙勫垝'); return; }
    const resDiv = document.getElementById('agent-results');
    resDiv.innerHTML = '<div style="text-align:center;padding:30px;color:#8b5cf6;">鈴?骞惰鎵ц鎵€鏈変换鍔′腑...</div>';

    try {
        const res = await fetch(`${API_BASE}/agent/execute_all`, {
            method:'POST', headers:{'Content-Type':'application/json'},
            body: JSON.stringify({
                task: _agentPlanData.original_task,
                agent_tasks: _agentPlanData.tasks,
                use_rag: document.getElementById('agent-rag-toggle').checked
            })
        });
        if (!res.ok) { const e=await res.json().catch(()=>({})); throw new Error(e.error||`${res.status}`); }
        const data = await res.json();

        const annotations = {};
        (data.results || []).forEach(r => { annotations[r.task_name] = {label: r.output.substring(0,30), ms: 0}; });
        document.getElementById('agent-container').innerHTML = renderGraphHtml(_agentPlanData.graph_structure, annotations);

        let html = '<div style="border:1px solid #e2e8f0;border-radius:8px;padding:15px;margin-top:10px;">';
        html += '<h4 style="margin:0 0 10px 0;color:#7c3aed;">骞惰鎵ц缁撴灉</h4>';
        html += `<div style="font-size:13px;color:#64748b;margin-bottom:10px;">鈴?${data.total_duration_ms}ms | 馃獧 ${data.total_tokens} tokens</div>`;
        html += '<table style="width:100%"><thead><tr style="background:#f5f3ff;"><th>浠诲姟</th><th>杈撳嚭</th><th style="width:100px;">鑰楁椂</th><th style="width:60px;">Token</th></tr></thead><tbody>';
        (data.results || []).forEach(r => {
            html += '<tr><td style="padding:8px;font-weight:bold;">' + escapeHtml(r.task_name) + '</td>';
            html += '<td style="padding:8px;font-size:13px;">' + escapeHtml(r.output) + '</td>';
            html += '<td style="padding:4px;font-size:11px;color:#94a3b8;text-align:right;">' + (r.duration_ms||0) + 'ms</td>';
            html += '<td style="padding:4px;font-size:11px;color:#94a3b8;text-align:right;">' + (r.tokens||0) + '</td></tr>';
        });
        html += '</tbody></table></div>';
        if (data.final_answer) {
            html += '<div style="border:1px solid #d1fae5;background:#ecfdf5;border-radius:8px;padding:15px;margin-top:10px;">';
            html += '<strong style="color:#059669;">鏈€缁堢瓟妗堬細</strong><br>' + escapeHtml(data.final_answer) + '</div>';
        }
        resDiv.innerHTML = html;
    } catch (e) {
        resDiv.innerHTML = '<div style="color:#e94560;padding:20px;">鉂?' + escapeHtml(e.message) + '</div>';
    }
}

async function runAgentPlan() {
    const task = document.getElementById('agent-input').value.trim();
    if (!task) { showToast('璇疯緭鍏ヤ换鍔?); return; }
    const viz = document.getElementById('agent-viz');
    const container = document.getElementById('agent-container');
    const detail = document.getElementById('agent-plan-detail');
    const results = document.getElementById('agent-results');

    viz.style.display = 'block'; detail.style.display = 'none';
    document.getElementById('agent-graph-title').textContent = '馃 瑙勫垝涓?..';
    container.innerHTML = '<div style="text-align:center;color:#94a3b8;padding:30px;">鈴?LLM 瑙勫垝涓?..</div>';
    results.innerHTML = '';

    try {
        const res = await fetch(`${API_BASE}/agent/plan`, {
            method: 'POST', headers: {'Content-Type':'application/json'},
            body: JSON.stringify({task, use_rag: document.getElementById('agent-rag-toggle').checked, use_routing: document.getElementById('agent-routing-toggle').checked})
        });
        if (!res.ok) { const e=await res.json().catch(()=>({})); throw new Error(e.error||`${res.status}`); }
        const data = await res.json();
        _agentPlanData = data;

        document.getElementById('agent-graph-title').textContent = `馃搵 ${escapeHtml(data.original_task)}`;
        container.innerHTML = renderGraphHtml(data.graph_structure, {});

        let html = `<div style="border:1px solid #e2e8f0;border-radius:8px;padding:15px;margin-top:10px;">
            <table style="width:100%;border-collapse:collapse;">
            <thead><tr style="background:#f5f3ff;">
                <th style="padding:8px;border:1px solid #e2e8f0;">浠诲姟</th>
                <th style="padding:8px;border:1px solid #e2e8f0;">宸ュ叿</th>
                <th style="padding:8px;border:1px solid #e2e8f0;">渚濊禆</th>
                <th style="padding:8px;border:1px solid #e2e8f0;">杈撳叆璇存槑</th>
            </tr></thead>
            <tbody>`;
        data.tasks.forEach(t => {
            html += `<tr><td style="padding:8px;border:1px solid #e2e8f0;"><strong>${escapeHtml(t.name)}</strong><br><span style="font-size:12px;color:#64748b;">${escapeHtml(t.description)}</span></td>
                <td style="padding:8px;border:1px solid #e2e8f0;"><span style="background:#ede9fe;padding:2px 8px;border-radius:4px;font-size:12px;">${escapeHtml(t.tool)}</span></td>
                <td style="padding:8px;border:1px solid #e2e8f0;font-size:12px;">${(t.depends_on||[]).join(', ') || '鏃?}</td>
                <td style="padding:8px;border:1px solid #e2e8f0;font-size:12px;color:#475569;">${escapeHtml(t.input_template)}</td></tr>`;
        });
        html += `</tbody></table></div>`;
        html += `<button class="btn" onclick="agentStepExecute()" style="background:#8b5cf6;color:white;margin-top:10px;padding:12px;width:100%;">鈻?寮€濮嬫墽琛?/button>`;
        results.innerHTML = html;
        detail.style.display = 'block';
    } catch (e) {
        container.innerHTML = `<div style="color:#e94560;text-align:center;padding:20px;">${e.message}</div>`;
    }
}

let _agentSessionId = null;
let _agentAllResults = [];

async function agentStepExecute() {
    if (!_agentPlanData) { alert('璇峰厛瑙勫垝'); return; }
    _agentSessionId = null;
    _agentAllResults = [];
    await agentFetchAndShow(true);
}

async function agentNextBatch() {
    document.querySelectorAll('#agent-results-table .btn, #agent-results-table span').forEach(b => b.remove());
    const el = document.createElement('div');
    el.id = 'batch-loading';
    el.style.cssText = 'text-align:center;padding:20px;color:#8b5cf6;';
    el.textContent = '鈴?鎵ц涓?..';
    document.getElementById('agent-results-table').appendChild(el);
    await agentFetchAndShow(false);
}

async function agentFetchAndShow(isFirst) {
    const resDiv = document.getElementById('agent-results');
    if (isFirst) {
        // 淇濈暀涓婃柟瑙勫垝琛ㄦ牸锛屼笅鏂硅拷鍔犵粨鏋滃尯鍩?
        let execDiv = document.createElement('div');
        execDiv.id = 'agent-exec-area';
        execDiv.innerHTML = '<div style="text-align:center;padding:30px;color:#8b5cf6;">鈴?鎵ц涓?..</div>';
        resDiv.appendChild(execDiv);
    }

    try {
        const url = isFirst ? '/api/agent/execute' : '/api/agent/next';
        const body = isFirst
            ? JSON.stringify({task: _agentPlanData.original_task, agent_tasks: _agentPlanData.tasks, use_rag: document.getElementById('agent-rag-toggle').checked})
            : JSON.stringify({session_id: _agentSessionId});

        const res = await fetch(url, {method:'POST', headers:{'Content-Type':'application/json'}, body});
        const data = await res.json();
        if (!res.ok) { throw new Error(data.error || '澶辫触'); }

        if (isFirst) _agentSessionId = data.session_id;
        (data.results || []).forEach(r => _agentAllResults.push(r));

        const annotations = {};
        _agentAllResults.forEach(r => { annotations[r.task_name] = {label: r.output.substring(0,30), ms: 0}; });
        document.getElementById('agent-container').innerHTML = renderGraphHtml(_agentPlanData.graph_structure, annotations);

        let html = '<div id="agent-results-table" style="border:1px solid #e2e8f0;border-radius:8px;padding:15px;margin-top:10px;">';
        html += '<h4 style="margin:0 0 10px 0;color:#7c3aed;">鎵ц缁撴灉</h4>';
        html += '<table style="width:100%"><thead><tr style="background:#f5f3ff;"><th>浠诲姟</th><th>杈撳嚭</th><th style="width:70px;">鑰楁椂/Token</th></tr></thead><tbody>';
        _agentAllResults.forEach(r => {
            const isSkipped = r.output && r.output.includes('⏭️');
            html += `<tr style="${isSkipped ? 'opacity:0.5;text-decoration:line-through;' : ''}">
                <td style="padding:8px;font-weight:bold;">${escapeHtml(r.task_name)}</td>
                <td style="padding:8px;font-size:13px;${isSkipped ? 'color:#94a3b8;' : ''}">${escapeHtml(r.output)}</td>
                <td style="padding:4px;font-size:11px;color:#94a3b8;text-align:right;">${(r.duration_ms||0)}ms | ${(r.tokens||0)}t</td></tr>`;
        });
        html += '</tbody></table>';

        // 鏇挎崲鎴栬拷鍔犲埌宸叉湁缁撴灉
        if (isFirst) {
            html += '<div style="padding:10px;text-align:center;">'
                + (data.has_next
                    ? '<button class="btn" onclick="agentNextBatch()" style="background:#8b5cf6;color:white;width:100%;padding:12px;">鈻?涓嬩竴姝?(' + _agentAllResults.length + '/' + _agentPlanData.tasks.length + ')</button>'
                    : '<span style="color:#10b981;font-weight:bold;">鉁?鍏ㄩ儴瀹屾垚</span>')
                + '</div></div>';
            let existArea = document.getElementById('agent-exec-area');
            if (existArea) {
                existArea.innerHTML = html;
            } else {
                let div = document.createElement('div');
                div.id = 'agent-exec-area';
                div.innerHTML = html;
                resDiv.appendChild(div);
            }
        } else {
            document.querySelectorAll('#batch-loading').forEach(el => el.remove());
            const tbody = document.querySelector('#agent-results-table tbody');
            if (tbody) {
                (data.results || []).forEach(r => {
                    let row = document.createElement('tr');
                    row.innerHTML = '<td style="padding:8px;font-weight:bold;">' + escapeHtml(r.task_name) + '</td>'
                        + '<td style="padding:8px;font-size:13px;">' + escapeHtml(r.output) + '</td>'
                        + '<td style="padding:4px;font-size:10px;color:#94a3b8;text-align:center;line-height:1.6;">' + (r.duration_ms||0) + 'ms<br>' + (r.tokens||0) + 't</td>';
                    tbody.appendChild(row);
                });
            }
            let footer = document.createElement('div');
            footer.style.cssText = 'padding:10px;text-align:center;';
            if (data.has_next) {
                footer.innerHTML = '<button class="btn" onclick="agentNextBatch()" style="background:#8b5cf6;color:white;width:100%;padding:12px;">鈻?涓嬩竴姝?(' + _agentAllResults.length + '/' + _agentPlanData.tasks.length + ')</button>';
            } else {
                footer.innerHTML = '<span style="color:#10b981;font-weight:bold;">鉁?鍏ㄩ儴瀹屾垚</span>';
            }
            document.querySelector('#agent-results-table').appendChild(footer);
        }
    } catch (e) {
        resDiv.innerHTML = '<div style="color:#e94560;padding:20px;">鉂?' + escapeHtml(e.message) + '</div>';
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
            if (confirm(`妫€娴嬪埌 ${files.length} 涓枃浠讹紝鏄惁鎵归噺涓婁紶锛焋)) {
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
    document.getElementById('chunk-strategy').addEventListener('change', updateChunkStrategyDesc);
    updateChunkStrategyDesc();
    
    document.getElementById('bm25-query').addEventListener('keypress', (e) => { if (e.key === 'Enter') bm25Search(); });
    document.getElementById('vector-query').addEventListener('keypress', (e) => { if (e.key === 'Enter') vectorSearch(); });
    document.getElementById('compare-query').addEventListener('keypress', (e) => { if (e.key === 'Enter') compareSearch(); });
    document.getElementById('pageindex-query').addEventListener('keypress', (e) => { if (e.key === 'Enter') pageindexSearch(); });
    
    fetchStats();
    
    const savedSessionId = localStorage.getItem('chat_session_id');
    if (savedSessionId) { currentSessionId = savedSessionId; loadSession(savedSessionId); }
    
    loadLangGraphInfo();
    
    initDarkMode();
});




