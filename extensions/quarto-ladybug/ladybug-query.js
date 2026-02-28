"use strict";

// ladybug-query.js — Execute Cypher queries against ladybug-rs /api/v1/cypher

function fetch_with_timeout(url, options, timeout = 10000) {
    return Promise.race([
        fetch(url, options),
        new Promise((_, reject) =>
            setTimeout(() => reject(new Error('timeout')), timeout)
        ),
    ]);
}

function query_text(block) {
    return block.querySelector("code").innerText;
}

function render_table(data) {
    if (!data || !data.columns || !data.rows || data.rows.length === 0) {
        return '<p class="ladybug-empty">No results</p>';
    }

    let html = '<table class="ladybug-result-table">';
    html += '<thead><tr>';
    for (const col of data.columns) {
        html += '<th>' + escape_html(col) + '</th>';
    }
    html += '</tr></thead><tbody>';

    for (const row of data.rows) {
        html += '<tr>';
        for (const col of data.columns) {
            const val = row[col];
            const display = val === null || val === undefined
                ? '<span class="null">null</span>'
                : escape_html(String(val));
            html += '<td>' + display + '</td>';
        }
        html += '</tr>';
    }

    html += '</tbody></table>';
    return html;
}

function escape_html(str) {
    const div = document.createElement('div');
    div.appendChild(document.createTextNode(str));
    return div.innerHTML;
}

function render_stats(data) {
    if (!data || !data.stats) return '';
    const s = data.stats;
    const parts = [];
    if (s.nodes_created) parts.push(s.nodes_created + ' nodes created');
    if (s.relationships_created) parts.push(s.relationships_created + ' rels created');
    if (s.properties_set) parts.push(s.properties_set + ' props set');
    if (s.execution_time_ms !== undefined) parts.push(s.execution_time_ms + ' ms');
    if (parts.length === 0) return '';
    return '<div class="ladybug-stats">' + parts.join(' | ') + '</div>';
}

function run_ladybug_query(pre_block) {
    var result_block = pre_block.querySelector(".ladybug-result");
    if (!result_block) {
        result_block = document.createElement('div');
        result_block.className = 'ladybug-result';
        pre_block.parentNode.insertBefore(result_block, pre_block.nextSibling);
    }

    const cypher = query_text(pre_block);
    const endpoint = pre_block.getAttribute('data-ladybug-endpoint')
        || 'http://127.0.0.1:8080';

    result_block.innerHTML = '<p class="ladybug-running">Running query...</p>';

    fetch_with_timeout(endpoint + '/api/v1/cypher', {
        headers: {
            'Content-Type': 'application/json',
            'Accept': 'application/json',
        },
        method: 'POST',
        body: JSON.stringify({ query: cypher }),
    })
    .then(response => {
        if (!response.ok) {
            throw new Error('HTTP ' + response.status + ': ' + response.statusText);
        }
        return response.json();
    })
    .then(data => {
        let html = '';
        if (data.error) {
            html = '<div class="ladybug-error">' + escape_html(data.error) + '</div>';
        } else {
            html = render_table(data) + render_stats(data);
        }
        result_block.innerHTML = html;
    })
    .catch(error => {
        result_block.innerHTML =
            '<div class="ladybug-error">ladybug-rs: ' + escape_html(error.message) + '</div>';
    });
}

// Process all ladybug query blocks
document.addEventListener('DOMContentLoaded', function() {
    var blocks = Array.from(document.querySelectorAll(".ladybug-query"));

    blocks.forEach(function(pre_block) {
        // Add run button
        var buttons = pre_block.querySelector(".buttons");
        if (!buttons) {
            buttons = document.createElement('div');
            buttons.className = 'buttons';
            pre_block.insertBefore(buttons, pre_block.firstChild);
        }

        var runButton = document.createElement('button');
        runButton.className = 'run-ladybug-button play-button';
        runButton.title = 'Run Cypher query against ladybug-rs';
        runButton.setAttribute('aria-label', runButton.title);

        buttons.insertBefore(runButton, buttons.firstChild);
        runButton.addEventListener('click', function() {
            run_ladybug_query(pre_block);
        });
    });
});
