/**
 * 主入口 - 应用初始化和协调
 */

const App = {
    elements: {},
    init() {
        this.elements = {
            tabs: document.querySelectorAll('.tab'),
            panels: {
                upload: document.getElementById('upload-panel'),
                search: document.getElementById('search-panel'),
                test: document.getElementById('test-panel')
            },
            stats: {
                docs: document.getElementById('stat-docs'),
                dim: document.getElementById('stat-dim'),
                bm25: document.getElementById('stat-bm25'),
                collection: document.getElementById('stat-collection')
            }
        };
        this.bindEvents();
        Upload.init();
        Search.init();
        Test.init();
        this.loadStats();
    },

    bindEvents() {
        this.elements.tabs.forEach(tab => {
            tab.addEventListener('click', () => {
                const target = tab.dataset.tab;
                this.switchTab(target);
            });
        });
    },

    switchTab(name) {
        this.elements.tabs.forEach(t => t.classList.toggle('active', t.dataset.tab === name));
        Object.entries(this.elements.panels).forEach(([key, panel]) => panel.classList.toggle('hidden', key !== name));
    },

    async loadStats() {
        try {
            const stats = await Api.getStats();
            this.elements.stats.docs.textContent = stats.total_documents;
            this.elements.stats.dim.textContent = stats.vector_size;
            this.elements.stats.bm25.textContent = stats.bm25_chunks || '--';
            this.elements.stats.collection.textContent = stats.collection_name;
        } catch (err) { console.error('加载统计失败:', err); }
    }
};

document.addEventListener('DOMContentLoaded', () => App.init());