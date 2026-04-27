/**
 * 测试模块
 * 
 * 处理精准度测试、报告显示
 */

const Test = {
    elements: {},
    isRunning: false,

    init() {
        this.elements = {
            btn: document.getElementById('btn-test'),
            report: document.getElementById('test-report')
        };

        this.bindEvents();
    },

    bindEvents() {
        this.elements.btn.addEventListener('click', () => this.runTest());
    },

    async runTest() {
        if (this.isRunning) return;

        this.isRunning = true;
        this.elements.btn.disabled = true;
        this.elements.btn.textContent = '测试中...';
        this.elements.report.innerHTML = '<div class="loading">正在运行测试...</div>';

        try {
            const report = await Api.runPrecisionTest();
            this.showReport(report);
        } catch (err) {
            this.showError(`测试失败: ${err.message}`);
        }

        this.isRunning = false;
        this.elements.btn.disabled = false;
        this.elements.btn.textContent = '运行精准度测试';
        App.loadStats();
    },

    showReport(report) {
        const summaryHtml = `
            <div class="test-summary">
                <div class="test-stat">
                    <span class="test-stat-value">${report.total_tests}</span>
                    <span class="test-stat-label">总测试数</span>
                </div>
                <div class="test-stat">
                    <span class="test-stat-value">${report.passed_tests}</span>
                    <span class="test-stat-label">通过数</span>
                </div>
                <div class="test-stat">
                    <span class="test-stat-value">${(report.precision_score * 100).toFixed(1)}%</span>
                    <span class="test-stat-label">精准度</span>
                </div>
                <div class="test-stat">
                    <span class="test-stat-value">${report.average_position.toFixed(2)}</span>
                    <span class="test-stat-label">平均排名</span>
                </div>
            </div>
        `;

        const itemsHtml = report.results.map(r => `
            <div class="test-item ${r.passed ? 'passed' : 'failed'}">
                <div class="test-header">
                    <span class="test-status ${r.passed ? 'passed' : 'failed'}">
                        ${r.passed ? '✓ 通过' : '✗ 失败'}
                    </span>
                    <span>${r.test_case.description}</span>
                </div>
                <div class="test-query">
                    查询: "${r.test_case.query}"
                </div>
                <div class="test-details">
                    ${r.found 
                        ? `找到位置: #${r.position} | 分数: ${(r.score * 100).toFixed(1)}%` 
                        : '未找到相关文档'}
                </div>
            </div>
        `).join('');

        this.elements.report.innerHTML = summaryHtml + itemsHtml;
    },

    showError(message) {
        this.elements.report.innerHTML = `
            <div class="message error">${message}</div>
        `;
    }
};