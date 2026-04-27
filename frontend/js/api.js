/**
 * API 请求封装
 * 
 * 所有后端 API 调用的统一入口
 */

const API_BASE_URL = '/api';

const Api = {
    async uploadFile(file) {
        const formData = new FormData();
        formData.append('file', file);
        const response = await fetch(`${API_BASE_URL}/upload`, {
            method: 'POST',
            body: formData
        });
        if (!response.ok) {
            const error = await response.json();
            throw new Error(error.error || '上传失败');
        }
        return await response.json();
    },

    async searchVector(query, topK = 5) {
        const response = await fetch(`${API_BASE_URL}/search/vector`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ query, top_k: topK })
        });
        if (!response.ok) throw new Error('向量搜索失败');
        return await response.json();
    },

    async searchBM25(query, topK = 5) {
        const response = await fetch(`${API_BASE_URL}/search/bm25`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ query, top_k: topK })
        });
        if (!response.ok) throw new Error('BM25搜索失败');
        return await response.json();
    },

    async searchHybrid(query, topK = 5) {
        const response = await fetch(`${API_BASE_URL}/search/hybrid`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ query, top_k: topK })
        });
        if (!response.ok) throw new Error('混合搜索失败');
        return await response.json();
    },

    async compareSearch(query, topK = 5) {
        const response = await fetch(`${API_BASE_URL}/search/compare`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ query, top_k: topK })
        });
        if (!response.ok) throw new Error('对比搜索失败');
        return await response.json();
    },

    async getStats() {
        const response = await fetch(`${API_BASE_URL}/stats`);
        if (!response.ok) throw new Error('获取统计失败');
        return await response.json();
    },

    async clearAll() {
        const response = await fetch(`${API_BASE_URL}/clear`, { method: 'POST' });
        if (!response.ok) throw new Error('清空失败');
        return await response.json();
    },

    async runPrecisionTest() {
        const response = await fetch(`${API_BASE_URL}/test/precision`, { method: 'POST' });
        if (!response.ok) throw new Error('测试失败');
        return await response.json();
    }
};