// The bundled XY chart renderer accepts series names but does not draw a legend.
// Preserve those names before rendering and include the legend inside the SVG so
// it also appears in fullscreen and print output.
(function () {
    const sources = new WeakMap();
    const ns = 'http://www.w3.org/2000/svg';

    function capture(pre) {
        if (pre.hasAttribute('data-processed')) return;
        const source = pre.textContent;
        if (!/^\s*xychart(?:-beta)?\b/m.test(source)) return;
        const series = Array.from(source.matchAll(/^\s*(line|bar)\s*(?:"([^"\n]*)"|([^\[\n]+?))?\s*\[/gm), match => ({
            type: match[1],
            name: (match[2] || match[3] || '').trim()
        }));
        sources.set(pre, series);
    }

    function element(tag, attrs, parent) {
        const node = document.createElementNS(ns, tag);
        Object.entries(attrs).forEach(([key, value]) => node.setAttribute(key, value));
        parent.appendChild(node);
        return node;
    }

    function render(pre) {
        const series = sources.get(pre);
        const svg = pre.querySelector('svg');
        if (!series || !svg || svg.querySelector('.mdr-plot-legend')) return;
        const named = series.map((item, index) => ({...item, index})).filter(item => item.name);
        if (!named.length) return;

        const box = svg.viewBox.baseVal;
        const left = box.x + 16;
        const width = Math.max(80, box.width - 32);
        const top = box.y + box.height + 18;
        const legend = element('g', {
            class: 'mdr-plot-legend', role: 'group', 'aria-label': 'Chart legend',
            'font-size': 14, fill: getComputedStyle(pre).color
        }, svg);
        let x = 0;
        let y = 0;
        let rowHeight = 24;
        named.forEach(item => {
            const plot = svg.querySelector(`.${item.type}-plot-${item.index} ${item.type === 'line' ? 'path' : 'rect'}`);
            if (!plot) return;
            const color = getComputedStyle(plot)[item.type === 'line' ? 'stroke' : 'fill'];
            const group = element('g', {'data-series-index': item.index}, legend);
            const text = element('text', {x: 32, y: 0}, group);
            // Use textContent, never HTML, for document-supplied series names.
            text.textContent = item.name;
            const itemWidth = Math.min(width, 32 + text.getComputedTextLength() + 24);
            if (x && x + itemWidth > width) {
                x = 0;
                y += rowHeight;
                rowHeight = 24;
            }
            text.textContent = '';
            let line = element('tspan', {x: 32, dy: 0}, text);
            let lines = 1;
            // Wrap long names without shrinking the chart or clipping its labels.
            for (const word of item.name.split(/\s+/)) {
                const previous = line.textContent;
                line.textContent = previous ? previous + ' ' + word : word;
                if (previous && line.getComputedTextLength() > width - 32) {
                    line.textContent = previous;
                    line = element('tspan', {x: 32, dy: 18}, text);
                    line.textContent = word;
                    lines++;
                }
            }
            if (item.type === 'line') {
                element('line', {x1: 0, x2: 22, y1: -5, y2: -5, stroke: color, 'stroke-width': 3}, group);
            } else {
                element('rect', {x: 0, y: -13, width: 22, height: 12, fill: color}, group);
            }
            group.setAttribute('transform', `translate(${left + x}, ${top + y})`);
            rowHeight = Math.max(rowHeight, lines * 18 + 6);
            x += itemWidth;
        });
        const bounds = legend.getBBox();
        const right = Math.max(box.x + box.width, bounds.x + bounds.width + 16);
        const bottom = Math.max(box.y + box.height, bounds.y + bounds.height + 16);
        svg.setAttribute('viewBox', [box.x, box.y, right - box.x, bottom - box.y].join(' '));
    }

    window.mdrMermaidLegends = {capture, render};
})();
