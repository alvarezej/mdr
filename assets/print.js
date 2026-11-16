(function () {
    window.mdrExportPdf = function () {
        var button = document.getElementById('export-pdf-button');
        if (button.disabled) return;
        var status = document.getElementById('pdf-export-status');
        if (!window.ipc || !window.ipc.postMessage) {
            status.hidden = false;
            status.textContent = 'PDF export is available in the mdr viewer.';
            return;
        }
        button.disabled = true;
        status.hidden = false;
        status.textContent = 'Exporting PDF…';
        var landscape = document.getElementById('pdf-orientation').value === 'landscape';
        window.ipc.postMessage(landscape ? '__MDR_IPC_EXPORT_PDF_LANDSCAPE__' : '__MDR_IPC_EXPORT_PDF__');
    };

    window.mdrFinishPdfExport = function (result) {
        document.getElementById('export-pdf-button').disabled = false;
        var status = document.getElementById('pdf-export-status');
        status.hidden = !!result.cancelled;
        status.textContent = result.error ? 'Export failed: ' + result.error :
            result.path ? 'Saved PDF: ' + result.path : '';
    };

    window.mdrRefreshPrintToc = function () {
        var toc = document.querySelector('.print-toc');
        var entries = document.querySelector('.sidebar ul');
        toc.querySelector('ul').innerHTML = entries.innerHTML;
        toc.hidden = !entries.children.length || document.body.classList.contains('mdr-no-print-toc');
        var title = document.querySelector('.content h1, .content h2, .content h3');
        if (title) document.title = title.textContent;
    };

    // Keep the promise from the current render: rendering again while Mermaid
    // is already working can mark unfinished diagrams as processed.
    window.mdrPreparePrint = async function () {
        if (document.readyState === 'loading') {
            await new Promise(function (resolve) {
                document.addEventListener('DOMContentLoaded', resolve, {once: true});
            });
        }
        await window.mdrMermaidReady;
        if (window.mdrMermaidError) throw new Error(window.mdrMermaidError);
        if (document.fonts) await document.fonts.ready;
        await Promise.all(Array.from(document.images).map(function (img) {
            if (img.complete) return Promise.resolve();
            return new Promise(function (resolve) {
                img.addEventListener('load', resolve, {once: true});
                img.addEventListener('error', resolve, {once: true});
            });
        }));
        window.mdrPrepareTables();
        window.mdrRefreshPrintToc();
    };

    var printing = false;
    window.mdrPrint = async function () {
        if (printing) return;
        printing = true;
        try {
            await window.mdrPreparePrint();
            if (window.ipc && window.ipc.postMessage) {
                window.ipc.postMessage('__MDR_IPC_PRINT__');
            } else {
                window.print();
            }
        } catch (error) {
            window.alert('Could not prepare PDF: ' + error.message);
        } finally {
            printing = false;
        }
    };

    window.addEventListener('beforeprint', window.mdrRefreshPrintToc);
    window.mdrRefreshPrintToc();
})();
