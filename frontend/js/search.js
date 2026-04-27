/**
 * 搜索模块
 * 
 * 处理向量搜索、结果显示
 */

const Search = {
    elements: {},
    currentMode: 'hybrid',
    isSearching: false,

    init() {
        this.elements = {
            input: document.getElementById('search-input'),
            topK: document.getElementById('search-k'),
            btn: document.getElementById('btn-search'),
            results: document.getElementById('search-results'),
            modeButtons: document.querySelectorAll('.mode-btn')
        };
        this.bindEvents();
    },

    bindEvents() {
        this.elements.btn.addEventListener('click', () => this.doSearch());
        this.elements.input.addEventListener('keypress', (e) => {
            if (e.key === 'Enter') this.doSearch();
        });
        
        this.elements.modeButtons.forEach(btn => {
            btn.addEventListener('click', () => {
                this.elements.modeButtons.forEach(b => b.classList.remove('active'));
                btn.classList.add('active');
                this.currentMode = btn.dataset.mode;
            });
        });
    },

    async doSearch() {
        if (this.isSearching) return;
        const query = this.elements.input.value.trim();
        if (!query) {
            this.showResults([]);
            this.showMessage('error', '请输入搜索内容');
            return;
        }
        const topK = parseInt(this.elements.topK.value) || 5;

        this.isSearching = true;
        this.elements.btn.disabled = true;
        this.showMessage('info', `正在${this.getModeLabel()}搜索...`);

        try {
            let response;
            switch (this.currentMode) {
                case 'vector': response = await Api.searchVector(query, topK); break;
                case 'bm25': response = await Api.searchBM25(query, topK); break;
                case 'hybrid': response = await Api.searchHybrid(query, topK); break;
                case 'compare':
                    response = await Api.compareSearch(query, topK);
                    this.showCompareResults(response);
                    this.isSearching = false;
                    this.elements.btn.disabled = false;
                    return;
            }
            this.showResults(response.results, response.mode);
            if (response.results.length === 0) this.showMessage('info', '未找到相关文档');
        } catch (err) {
            this.showMessage('error', `搜索失败: ${err.message}`);
        }

        this.isSearching = false;
        this.elements.btn.disabled = false;
    },

    getModeLabel() {
        return { 'vector': '向量', 'bm25': 'BM25', 'hybrid': '混合', 'compare': '对比' }[this.currentMode] || '混合';
    },

    showResults(results, mode = 'vector') {
        if (results.length === 0) {
            this.elements.results.innerHTML = '<div class="empty-state">暂无搜索结果</div>';
            return;
        }
        const html = results.map((r, i) => `
            <div class="result-item">
                <div class="result-header">
                    <span class="result-id">ID: ${r.id || '-'}</span>
                    <span class="result-mode">${mode}</span>
                    <span class="result-score">${(r.score * 100).toFixed(1)}%</span>
                </div>
                <div class="result-content">${this.escapeHtml(r.content.substring(0, 400))}${r.content.length > 400 ? '...' : ''}</div>
                <div class="result-meta">来源: ${r.source || '-'}</div>
            </div>
        `).join('');
        this.elements.results.innerHTML = html;
    },

    showCompareResults(response) {
        const { comparison, vector_results, bm25_results, hybrid_results } = response;
        const html = `
            <div class="compare-summary">
                <div class="compare-stat"><span class="compare-value">${(comparison.vector_top1_score * 100).toFixed(1)}%</span><span class="compare-label">向量 Top1</span></div>
                <div class="compare-stat"><span class="compare-value">${(comparison.bm25_top1_score * 100).toFixed(1)}%</span><span class="compare-label">BM25 Top1</span></div>
                <div class="compare-stat"><span class="compare-value">${(comparison.hybrid_top1_score * 100).toFixed(1)}%</span><span class="compare-label">混合 Top1</span></div>
                <div class="compare-stat"><span class="compare-value">${comparison.overlap_count}</span><span class="compare-label">重叠</span></div>
            </div>
            <div class="compare-sections">
                <div class="compare-section"><h4>向量 (${vector_results.length})</h4>${vector_results.slice(0,3).map(r=>`<div class="compare-item"><span>${(r.score*100).toFixed(1)}%</span>${r.content.substring(0,80)}...</div>`).join('')}</div>
                <div class="compare-section"><h4>BM25 (${bm25_results.length})</h4>${bm25_results.slice(0,3).map(r=>`<div class="compare-item"><span>${(r.score*100).toFixed(1)}%</span>${r.content.substring(0,80)}...</div>`).join('')}</div>
                <div class="compare-section"><h4>混合 (${hybrid_results.length})</h4>${hybrid_results.slice(0,3).map(r=>`<div class="compare-item"><span>${(r.score*100).toFixed(1)}%</span>${r.content.substring(0,80)}...</div>`).join('')}</div>
            </div>`;
        this.elements.results.innerHTML = html;
    },

    showMessage(type, text) { this.elements.results.innerHTML = `<div class="message ${type}">${text}</div>`; },
    escapeHtml(text) { const div = document.createElement('div'); div.textContent = text; return div.innerHTML; }
};

            if (response.results.length === 0) {
                this.showMessage('info', '未找到相关文档');
            }
        } catch (err) {
            this.showMessage('error', `搜索失败: ${err.message}`);
        }

        this.isSearching = false;
        this.elements.btn.disabled = false;
    },

    showResults(results) {
        if (results.length === 0) {
            this.elements.results.innerHTML = `
                <div class="empty-state">暂无搜索结果</div>
            `;
            return;
        }

        const html = results.map((r, i) => `
            <div class="result-item">
                <div class="result-header">
                    <span class="result-id">ID: ${r.id || '-'}</span>
                    <span class="result-score">${(r.score * 100).toFixed(1)}%</span>
                </div>
                <div class="result-content">
                    ${this.escapeHtml(r.content.substring(0, 400))}
                    ${r.content.length > 400 ? '...' : ''}
                </div>
                <div class="result-meta">
                    ${r.metadata ? JSON.stringify(r.metadata) : ''}
                </div>
            </div>
        `).join('');

        this.elements.results.innerHTML = html;
    },

    showMessage(type, text) {
        this.elements.results.innerHTML = `
            <div class="message ${type}">${text}</div>
        `;
    },

    escapeHtml(text) {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
};