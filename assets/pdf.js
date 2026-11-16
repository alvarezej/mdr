// Runs after rendering, while the document already has its physical page width.
window.mdrPreparePdf = function () {
    // Reserve the page-number column before the first pagination pass. Replacing
    // its placeholder later must not change label wrapping or TOC page breaks.
    document.querySelectorAll('.print-toc a').forEach(function (link, index) {
        var label = document.createElement('span');
        label.className = 'toc-label';
        label.textContent = link.textContent;
        var leader = document.createElement('span');
        leader.className = 'toc-leader';
        leader.setAttribute('aria-hidden', 'true');
        var page = document.createElement('span');
        page.className = 'toc-page';
        page.dataset.mdrTocIndex = String(index);
        page.textContent = '—';
        link.replaceChildren(label, leader, page);
    });

    document.querySelectorAll('.content table').forEach(function (table) {
        var columns = Array.from(table.rows).reduce(function (max, row) {
            return Math.max(max, Array.from(row.cells).reduce(function (sum, cell) {
                return sum + (cell.colSpan || 1);
            }, 0));
        }, 0);
        if (!columns) return;
        // Allocate more space to prose and URLs than to short numeric columns.
        // Using a square root limits the influence of extremely long values.
        var weights = Array(columns).fill(3);
        Array.from(table.rows).slice(0, 60).forEach(function (row) {
            var index = 0;
            Array.from(row.cells).forEach(function (cell) {
                var span = cell.colSpan || 1;
                var length = Math.min(160, cell.textContent.trim().length / span);
                for (var i = 0; i < span && index < columns; i++, index++) {
                    weights[index] = Math.max(weights[index], Math.sqrt(length + 4));
                }
            });
        });
        var total = weights.reduce(function (sum, value) { return sum + value; }, 0);
        Array.from(table.children).filter(function (el) { return el.tagName === 'COLGROUP'; })
            .forEach(function (el) { el.remove(); });
        var group = document.createElement('colgroup');
        weights.forEach(function (weight) {
            var column = document.createElement('col');
            column.style.width = (100 * weight / total) + '%';
            group.appendChild(column);
        });
        table.insertBefore(group, table.firstChild);
    });

    var width = document.querySelector('.content').getBoundingClientRect().width;
    var height = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--mdr-print-height')) * 96 / 25.4;
    document.querySelectorAll('.content pre.mermaid svg').forEach(function (svg) {
        var box = svg.viewBox.baseVal;
        if (!box.width || !box.height) return;
        var natural = parseFloat(svg.style.maxWidth) || box.width;
        var scale = Math.min(natural / box.width, width / box.width, height / box.height);
        svg.style.setProperty('width', (box.width * scale) + 'px', 'important');
        svg.style.setProperty('height', (box.height * scale) + 'px', 'important');
    });
};
