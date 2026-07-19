$(`
<div id="menu-UI">
    <div class="settings-page">
        <header class="settings-header">
            <div class="settings-header-main">
                <h1 class="settings-title">Lyrics Status</h1>
                <p class="settings-subtitle">Shows synced lyrics from Spotify in your Discord status.</p>
            </div>
            <div class="settings-header-meta">
                <span id="version" class="settings-version">v4</span>
            </div>
        </header>

        <main id="menu-contents" class="settings-content">
            <section class="settings-section">
                <h2 class="settings-name">Discord</h2>
                <div class="option form-row">
                    <label class="form-label" for="user-token">Token</label>
                    <div class="form-field">
                        <div class="form-field-inline">
                            <input type="text" id="user-token" class="text-input full-width-input" placeholder="Paste your Discord user token">
                            <button id="check-token" class="button"><span class="label">Check</span></button>
                        </div>
                    </div>
                </div>
            </section>

            <section class="settings-section">
                <h2 class="settings-name">Status</h2>
                <div class="option form-row">
                    <label class="checkbox-row" for="enable-timestamp">
                        <input type="checkbox" id="enable-timestamp" checked>
                        <span>Show playback timestamp</span>
                    </label>
                </div>
                <div class="option form-row">
                    <label class="checkbox-row" for="enable-label">
                        <input type="checkbox" id="enable-label" checked>
                        <span>Show label before lyrics</span>
                    </label>
                </div>
                <div class="option form-row">
                    <label class="checkbox-row" for="enable-autoclear">
                        <input type="checkbox" id="enable-autoclear" checked>
                        <span>Clear status on song switch</span>
                    </label>
                </div>
                <div class="option form-row">
                    <label class="form-label">Preview</label>
                    <div class="form-field">
                        <div id="status-preview" class="preview-bar">[2:17] Song lyrics - La-la-la</div>
                    </div>
                </div>
                <div class="divider"></div>
                <div class="option form-row">
                    <label class="checkbox-row" for="enable-advanced-swt">
                        <input type="checkbox" id="enable-advanced-swt">
                        <span>Advanced template</span>
                    </label>
                </div>
                <div id="advanced-swt" class="sub-settings hid">
                    <div class="option form-row">
                        <label class="form-label" for="custom-emoji">
                            Emoji
                            <span id="custom-emoji-help" class="help-btn">?</span>
                        </label>
                        <input style="width: 60px;" maxlength="4" id="custom-emoji" class="text-input" placeholder="...">
                    </div>
                    <div class="option form-row">
                        <label class="form-label" for="custom-status">
                            Template
                            <span id="custom-status-help" class="help-btn">?</span>
                        </label>
                        <div class="form-field">
                            <textarea rows="3" cols="40" id="custom-status" class="text-input textarea-input" placeholder="[{timestamp}] Song lyrics - {lyrics}"></textarea>
                            <small class="field-help">Placeholders: {lyrics}, {song_name}, {song_author}, {timestamp}</small>
                        </div>
                    </div>
                </div>
            </section>

            <section class="settings-section">
                <h2 class="settings-name">Timing</h2>
                <div class="option form-row">
                    <label class="form-label" for="send-time-offset">Offset</label>
                    <div class="form-field-inline">
                        <input type="text" id="send-time-offset" class="text-input" maxlength="4" value="500">
                        <span class="inline-text">ms</span>
                        <span id="send-time-offset-help" class="help-btn">?</span>
                    </div>
                </div>
                <div class="option form-row">
                    <label class="checkbox-row" for="enable-autooffset">
                        <input type="checkbox" id="enable-autooffset">
                        <span>Autooffset</span>
                    </label>
                </div>
                <div class="option form-row">
                    <label class="form-label" for="autooffset">Samples</label>
                    <div class="form-field-inline">
                        <input style="width: 60px;" id="autooffset" class="text-input" type="text" maxlength="2">
                        <span class="inline-text">requests</span>
                        <span id="autooffset-help" class="help-btn">?</span>
                    </div>
                </div>
            </section>

            <section class="settings-section">
                <h2 class="settings-name">Update</h2>
                <div class="option form-row">
                    <label class="checkbox-row" for="enable-autoupdate">
                        <input type="checkbox" id="enable-autoupdate">
                        <span>Automatic update checks</span>
                    </label>
                </div>
            </section>
        </main>
    </div>

    <!-- Dark mode toggle -->
    <button id="theme-toggle" class="theme-toggle" aria-label="Toggle dark mode">
        <svg class="icon-moon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/>
        </svg>
        <svg class="icon-sun" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/>
        </svg>
    </button>

<style>
        @import url('https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;500;600;700&display=swap');

        * { box-sizing: border-box; margin: 0; padding: 0; }

        /* ── Light theme (default) ──────────────────────── */
        :root {
            --bg:          #f5f3ef;
            --bg-surface:  #fff;
            --bg-inset:    #fff;
            --fg:          #2a2a2a;
            --fg-muted:    #666;
            --fg-dim:      #999;
            --border:      #2a2a2a;
            --border-soft: #ccc;
            --border-faint:#ddd;
            --accent:      #2a2a2a;
            --error:       #b44;
            --scrollbar:   #ccc;
            --scrolltrack: #f5f3ef;
        }

        /* ── Dark theme ─────────────────────────────────── */
        [data-theme="dark"] {
            --bg:          #1a1a1a;
            --bg-surface:  #242424;
            --bg-inset:    #161616;
            --fg:          #e0e0e0;
            --fg-muted:    #aaa;
            --fg-dim:      #555;
            --border:      #e0e0e0;
            --border-soft: #444;
            --border-faint:#333;
            --accent:      #e0e0e0;
            --error:       #e66;
            --scrollbar:   #444;
            --scrolltrack: #1a1a1a;
        }

        #menu-UI {
            position: fixed; inset: 0; width: 100%; height: 100%;
            background: var(--bg);
            z-index: 999; overflow-y: auto;
            font-family: 'JetBrains Mono', 'Courier New', monospace;
        }

        #menu-UI * {
            color: var(--fg);
            font-family: 'JetBrains Mono', 'Courier New', monospace;
        }

        .settings-page {
            max-width: 720px; margin: 0 auto; padding: 40px 24px 60px;
        }

        /* ── Header ─────────────────────────────────────── */
        .settings-header {
            display: flex; justify-content: space-between; align-items: flex-end;
            border-bottom: 3px solid var(--border); padding-bottom: 16px; margin-bottom: 0;
        }
        .settings-title {
            margin: 0; font-size: 28px; font-weight: 700; letter-spacing: -0.5px;
        }
        .settings-subtitle {
            margin: 6px 0 0; font-size: 12px; color: var(--fg-muted); line-height: 1.5;
        }
        .settings-version {
            font-size: 11px; font-weight: 600; padding: 4px 10px;
            border: 2px solid var(--border); letter-spacing: 1px;
        }

        /* ── Sections ───────────────────────────────────── */
        .settings-section {
            padding: 20px 0; border-bottom: 1px solid var(--border-faint);
        }
        .settings-section:last-of-type { border-bottom: none; }

        .settings-name {
            font-size: 14px; font-weight: 700; margin: 0 0 14px;
            text-transform: uppercase; letter-spacing: 1px;
        }

        .option { margin-top: 10px; }
        .form-row { display: flex; align-items: flex-start; gap: 12px; }
        .form-label {
            width: 120px; font-size: 12px; font-weight: 600; padding-top: 8px;
            color: var(--fg-muted);
        }
        .form-field { flex: 1; display: flex; flex-direction: column; gap: 4px; }
        .form-field-inline { display: inline-flex; align-items: center; gap: 8px; }
        .full-width-input { width: 100%; max-width: 100%; }
        .field-help { font-size: 11px; color: var(--fg-dim); }
        .inline-text { font-size: 11px; color: var(--fg-dim); }

        .checkbox-row {
            display: inline-flex; align-items: center; gap: 10px;
            font-size: 12px; font-weight: 500; cursor: pointer;
        }
        .checkbox-row input[type="checkbox"] {
            appearance: none; -webkit-appearance: none;
            width: 18px; height: 18px; border: 2px solid var(--border-soft);
            background: var(--bg-surface); cursor: pointer;
            position: relative; flex-shrink: 0;
        }
        .checkbox-row input[type="checkbox"]:checked {
            border-color: var(--accent); background: var(--accent);
        }
        .checkbox-row input[type="checkbox"]:checked::after {
            content: ''; position: absolute; top: 3px; left: 6px;
            width: 4px; height: 8px; border: solid var(--bg); border-width: 0 2px 2px 0;
            transform: rotate(45deg);
        }

        /* ── Inputs ─────────────────────────────────────── */
        .text-input, .textarea-input {
            border: 2px solid var(--border-soft); border-radius: 0;
            background: var(--bg-surface); color: var(--fg);
            padding: 8px 12px; font-size: 12px; outline: none;
            font-family: 'JetBrains Mono', 'Courier New', monospace;
        }
        .text-input:focus, .textarea-input:focus {
            border-color: var(--accent);
        }
        .textarea-input { resize: vertical; min-height: 70px; line-height: 1.5; }

        /* ── Buttons ────────────────────────────────────── */
        .button {
            min-width: 90px; height: 34px; padding: 0 16px;
            font-size: 12px; font-weight: 600; letter-spacing: 0.5px;
            border: 2px solid var(--border-soft); border-radius: 0;
            background: var(--bg-surface); color: var(--fg-muted);
            cursor: pointer; font-family: 'JetBrains Mono', 'Courier New', monospace;
            transition: all 0.15s;
        }
        .button:hover { border-color: var(--accent); color: var(--fg); }
        .button:active { transform: translate(1px, 1px); }
        .button.success { border-color: var(--accent); background: var(--accent); color: var(--bg); }
        .button.error { border-color: var(--error); color: var(--error); }
        .button .label { display: inline-block; transition: opacity 0.15s; }

        /* ── Preview ────────────────────────────────────── */
        .preview-bar {
            border: 2px solid var(--border-soft); border-radius: 0;
            padding: 8px 16px; background: var(--bg-surface);
            font-size: 12px; white-space: nowrap; display: inline-flex;
            align-items: center; color: var(--fg-muted);
        }

        /* ── Divider ────────────────────────────────────── */
        .divider { height: 1px; background: var(--border-faint); margin: 14px 0; }

        /* ── Sub-settings ───────────────────────────────── */
        .sub-settings {
            margin-top: 8px; padding: 14px 16px;
            border: 2px dashed var(--border-soft); background: var(--bg-inset);
        }

        /* ── Help button ────────────────────────────────── */
        .help-btn {
            display: inline-flex; align-items: center; justify-content: center;
            width: 18px; height: 18px; border: 1px solid var(--border-soft);
            font-size: 10px; font-weight: 700; color: var(--fg-dim);
            cursor: pointer; position: relative; top: 0; margin-left: 4px;
        }
        .help-btn:hover { border-color: var(--accent); color: var(--fg); }

        /* ── Modal ──────────────────────────────────────── */
        .modal {
            min-width: 300px; max-width: 600px; width: fit-content; height: fit-content;
            background: var(--bg-surface); top: 50%; left: 50%;
            transform: translate(-50%, -50%);
            border: 3px solid var(--border); font-size: 13px;
            z-index: 9999; position: absolute;
        }
        .modal * { user-select: none; }
        .modal > .top {
            width: 100%; height: 36px; background: var(--accent);
            display: flex; align-items: center; justify-content: space-between;
            padding: 0 10px;
        }
        .modal > .top > .title {
            font-size: 12px; font-weight: 600; letter-spacing: 1px;
            text-transform: uppercase; color: var(--bg);
        }
        .modal > .top > .close {
            width: 22px; height: 22px; border: 2px solid var(--bg);
            display: flex; align-items: center; justify-content: center;
            cursor: pointer; font-size: 12px; font-weight: 700; color: var(--bg);
        }
        .modal > .top > .close:hover { background: var(--bg); color: var(--accent); }
        .modal > .description {
            padding: 14px 16px; text-align: left; line-height: 1.6;
            font-size: 12px; color: var(--fg-muted);
        }
        .modal > .description strong { color: var(--fg); }

        /* ── Theme toggle button ────────────────────────── */
        .theme-toggle {
            position: fixed; bottom: 24px; right: 24px; z-index: 10000;
            width: 44px; height: 44px;
            border: 2px solid var(--border); border-radius: 0;
            background: var(--bg-surface); color: var(--fg);
            cursor: pointer; display: flex; align-items: center; justify-content: center;
            padding: 0;
        }
        .theme-toggle svg { width: 20px; height: 20px; }
        .theme-toggle .icon-sun { display: none; }
        [data-theme="dark"] .theme-toggle .icon-moon { display: none; }
        [data-theme="dark"] .theme-toggle .icon-sun { display: block; }
        .theme-toggle:hover { border-color: var(--accent); }

        .act { display: block; }
        .hid { display: none; }
    </style>
</div>
`).appendTo(document.body);

// ── Element refs ──────────────────────────────────────────────────────────────
let userTokenInput        = $("#user-token"),
    checkTokenButton      = $("#check-token"),
    enableTimestampCheckbox = $("#enable-timestamp"),
    enableLabelCheckbox   = $("#enable-label"),
    statusPreview         = $("#status-preview"),
    advancedSWT           = $("#advanced-swt"),
    enableAdvancedSWT     = $("#enable-advanced-swt"),
    customEmojiHelp       = $("#custom-emoji-help"),
    customEmoji           = $("#custom-emoji"),
    customStatusHelp      = $("#custom-status-help"),
    customStatus          = $("#custom-status"),
    sendTimeOffset        = $("#send-time-offset"),
    sendTimeOffsetHelp    = $("#send-time-offset-help"),
    enableAutooffset      = $("#enable-autooffset"),
    autooffset            = $("#autooffset"),
    autooffsetHelp        = $("#autooffset-help"),
    enableAutoupdate      = $("#enable-autoupdate"),
    enableAutoclear       = $("#enable-autoclear");

// ── Settings model ────────────────────────────────────────────────────────────
let settings = {
    credentials: { token: "", uuid: "" },
    view: {
        timestamp: true,
        label: true,
        autoClear: true,
        advanced: { enabled: false, customEmoji: "", customStatus: "[{timestamp}] [{lyrics}]" }
    },
    timings:  { sendTimeOffset: 500, enableAutooffset: true, autooffset: 3 },
    update:   { enableAutoupdate: true }
};

let settingsLoaded = false;

// ── Dark mode toggle ──────────────────────────────────────────────────────────
(function initTheme() {
    const saved = localStorage.getItem("ls-theme");
    if (saved === "dark") document.documentElement.setAttribute("data-theme", "dark");
})();

$("#theme-toggle").on("click", function () {
    const isDark = document.documentElement.getAttribute("data-theme") === "dark";
    if (isDark) {
        document.documentElement.removeAttribute("data-theme");
        localStorage.setItem("ls-theme", "light");
    } else {
        document.documentElement.setAttribute("data-theme", "dark");
        localStorage.setItem("ls-theme", "dark");
    }
});

// ── Event handlers ────────────────────────────────────────────────────────────
userTokenInput.change(() => {
    settings.credentials.token = userTokenInput.val().replace(/"/g, "");
    saveSettings();
});

checkTokenButton.click(() => {
    const label = checkTokenButton.find(".label");
    const originalText = label.text();
    checkTokenButton.removeClass("success error");
    label.css("opacity", 0);

    let valid = checkToken(settings.credentials.token);

    setTimeout(() => {
        checkTokenButton.addClass(valid ? "success" : "error");
        label.text(valid ? "OK" : "ERR").css("opacity", 1);

        setTimeout(() => {
            label.css("opacity", 0);
            setTimeout(() => {
                label.text(originalText).css("opacity", 1);
                checkTokenButton.removeClass("success error");
            }, 200);
        }, 3000);
    }, 200);
});

enableTimestampCheckbox.click(() => {
    settings.view.timestamp = enableTimestampCheckbox.prop("checked");
    saveSettings();
    statusPreview.text(getStatusString("La-la-la", 137000));
});

enableLabelCheckbox.click(() => {
    settings.view.label = enableLabelCheckbox.prop("checked");
    saveSettings();
    statusPreview.text(getStatusString("La-la-la", 137000));
});

enableAutoclear.click(() => {
    settings.view.autoClear = enableAutoclear.prop("checked");
    saveSettings();
});

enableAdvancedSWT.click(() => {
    const state = enableAdvancedSWT.prop("checked");
    settings.view.advanced.enabled = state;
    saveSettings();
    advancedSWT.toggleClass("hid").toggleClass("act");
    enableTimestampCheckbox.prop("disabled", state);
    enableLabelCheckbox.prop("disabled", state);
});

customEmojiHelp.click(() => modal("Help",
    "Add a unicode emoji before your status."
));

customEmoji.on("input", (e) => {
    e.preventDefault();
    settings.view.advanced.customEmoji = customEmoji.val();
    saveSettings();
});

customStatusHelp.click(() => modal("Help",
    "Template placeholders: {lyrics}, {song_name}, {song_author}, {timestamp}.<br>Status is cropped to 128 characters."
));

customStatus.on("input", (e) => {
    e.preventDefault();
    settings.view.advanced.customStatus = customStatus.val();
    saveSettings();
});

sendTimeOffset.on("input", (e) => {
    e.preventDefault();
    const value = +sendTimeOffset.val();
    if (isNaN(value)) {
        sendTimeOffset.css("color", "var(--error)");
        return;
    }
    sendTimeOffset.css("color", "inherit");
    settings.timings.sendTimeOffset = value;
    saveSettings();
});

sendTimeOffsetHelp.click(() => modal("Help",
    "Makes the status change slightly before the lyric line so it feels in sync.<br>Defined in milliseconds. Default is 500."
));

enableAutooffset.click(() => {
    settings.timings.enableAutooffset = enableAutooffset.prop("checked");
    saveSettings();
});

autooffset.on("input", (e) => {
    e.preventDefault();
    const value = +autooffset.val();
    if (isNaN(value)) { autooffset.css("color", "var(--error)"); return; }
    autooffset.css("color", "inherit");
    settings.timings.autooffset = value;
    saveSettings();
});

autooffsetHelp.click(() => modal("Help",
    "Measures Discord API response times and adjusts the offset for you automatically."
));

enableAutoupdate.click(() => {
    settings.update.enableAutoupdate = enableAutoupdate.prop("checked");
    saveSettings();
});

// ── Util ──────────────────────────────────────────────────────────────────────
function formatSeconds(s) {
    return (s - (s %= 60)) / 60 + (9 < s ? ":" : ":0") + s;
}

function getStatusString(lyrics, time) {
    return `${settings.view.timestamp ? "[" + formatSeconds((time / 1000).toFixed(0)) + "] " : ""}${settings.view.label ? "Song lyrics - " : ""}${lyrics}`;
}

function checkToken(token) {
    let success = true;
    $.get({
        url: "https://discordapp.com/api/v8/users/@me",
        headers: { "Authorization": token },
        async: false,
        statusCode: { 401: () => success = false }
    });
    return success;
}

function saveSettings() {
    if (!settingsLoaded) return console.error("Can't save before settings are loaded.");
    ws.send(JSON.stringify(settings));
}

function loadSettings(raw) {
    const loaded = JSON.parse(raw);
    settings = $.extend(true, settings, loaded);

    try {
        userTokenInput.val(settings.credentials.token);
        enableTimestampCheckbox.prop("checked", settings.view.timestamp);
        enableLabelCheckbox.prop("checked", settings.view.label);
        enableAutoclear.prop("checked", settings.view.autoClear !== false);
        if (settings.view.advanced.enabled) enableAdvancedSWT.click();
        customEmoji.val(settings.view.advanced.customEmoji);
        customStatus.html(settings.view.advanced.customStatus);
        statusPreview.text(getStatusString("La-la-la", 137000));
        sendTimeOffset.val(settings.timings.sendTimeOffset);
        enableAutooffset.prop("checked", settings.timings.enableAutooffset);
        autooffset.val(settings.timings.autooffset);
        enableAutoupdate.prop("checked", settings.update.enableAutoupdate);
        settingsLoaded = true;
    } catch (e) {
        console.error(e);
    }
}

function modal(title, description) {
    const w = $(`
    <div class="modal">
        <div class="top">
            <span class="title">${title}</span>
            <div class="close">X</div>
        </div>
        <div class="description">${description}</div>
    </div>`).appendTo(document.body);

    w.find(".close").click(() => w.remove());
}

// ── WebSocket ─────────────────────────────────────────────────────────────────
const ws = new WebSocket("ws://localhost:8999/ws");
ws.onmessage = (msg) => loadSettings(msg.data);
