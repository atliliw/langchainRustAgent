/**
 * 上传模块
 * 
 * 处理文件上传、拖拽、进度显示
 */

const Upload = {
    elements: {},
    isUploading: false,

    init() {
        this.elements = {
            zone: document.getElementById('upload-zone'),
            input: document.getElementById('file-input'),
            progress: document.getElementById('upload-progress'),
            result: document.getElementById('upload-result'),
            clearBtn: document.getElementById('btn-clear')
        };

        this.bindEvents();
    },

    bindEvents() {
        const { zone, input, clearBtn } = this.elements;

        zone.addEventListener('click', () => input.click());

        input.addEventListener('change', (e) => {
            if (e.target.files.length > 0) {
                this.handleFile(e.target.files[0]);
            }
        });

        zone.addEventListener('dragover', (e) => {
            e.preventDefault();
            zone.classList.add('dragover');
        });

        zone.addEventListener('dragleave', () => {
            zone.classList.remove('dragover');
        });

        zone.addEventListener('drop', (e) => {
            e.preventDefault();
            zone.classList.remove('dragover');
            if (e.dataTransfer.files.length > 0) {
                this.handleFile(e.dataTransfer.files[0]);
            }
        });

        clearBtn.addEventListener('click', () => this.clearAll());
    },

    async handleFile(file) {
        if (this.isUploading) return;

        const allowedTypes = ['txt', 'pdf', 'md', 'json', 'csv'];
        const ext = file.name.split('.').pop().toLowerCase();

        if (!allowedTypes.includes(ext)) {
            this.showResult('error', `不支持的文件类型: ${ext}`);
            return;
        }

        this.isUploading = true;
        this.showProgress();
        this.showResult('info', '正在上传和处理...');

        try {
            const result = await Api.uploadFile(file);

            if (result.success) {
                this.showResult('success', 
                    `✓ 成功处理 ${result.chunk_count} 个文档块`);
            } else {
                this.showResult('error', result.message);
            }
        } catch (err) {
            this.showResult('error', `上传失败: ${err.message}`);
        }

        this.hideProgress();
        this.isUploading = false;
        App.loadStats();
    },

    async clearAll() {
        if (!confirm('确定要清空所有文档吗？')) return;

        try {
            const result = await Api.clearAll();
            this.showResult('success', result.message);
            App.loadStats();
        } catch (err) {
            this.showResult('error', `清空失败: ${err.message}`);
        }
    },

    showProgress() {
        this.elements.progress.innerHTML = `
            <div class="progress-bar">
                <div class="progress-fill" style="width: 50%"></div>
            </div>
        `;
    },

    hideProgress() {
        this.elements.progress.innerHTML = '';
    },

    showResult(type, message) {
        this.elements.result.innerHTML = `
            <div class="message ${type}">${message}</div>
        `;
    }
};