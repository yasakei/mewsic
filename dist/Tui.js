"use strict";
var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {
    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }
    return new (P || (P = Promise))(function (resolve, reject) {
        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }
        function rejected(value) { try { step(generator["throw"](value)); } catch (e) { reject(e); } }
        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }
        step((generator = generator.apply(thisArg, _arguments || [])).next());
    });
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.Tui = void 0;
const promises_1 = require("node:readline/promises");
const node_process_1 = require("node:process");
const Settings_1 = require("./Settings");
const supportsAnsi = node_process_1.stdout.isTTY && !process.env.NO_COLOR;
function color(code, text) {
    if (!supportsAnsi)
        return text;
    return `\u001b[${code}m${text}\u001b[0m`;
}
function dim(text) {
    return color(90, text);
}
function cyan(text) {
    return color(96, text);
}
function green(text) {
    return color(92, text);
}
function yellow(text) {
    return color(93, text);
}
function magenta(text) {
    return color(95, text);
}
function bold(text) {
    return supportsAnsi ? `\u001b[1m${text}\u001b[0m` : text;
}
function clearScreen() {
    if (!node_process_1.stdout.isTTY) {
        console.clear();
        return;
    }
    node_process_1.stdout.write("\u001b[2J\u001b[H");
}
function stripAnsi(text) {
    return text.replace(/\u001b\[[0-9;]*m/g, "");
}
function visibleLength(text) {
    return stripAnsi(text).length;
}
function fit(text, width) {
    const plainLength = visibleLength(text);
    if (plainLength <= width)
        return text + " ".repeat(Math.max(0, width - plainLength));
    const ellipsis = "…";
    let visible = 0;
    let out = "";
    for (let i = 0; i < text.length; i++) {
        const char = text[i];
        if (char === "\u001b") {
            const match = text.slice(i).match(/^\u001b\[[0-9;]*m/);
            if (match) {
                out += match[0];
                i += match[0].length - 1;
                continue;
            }
        }
        if (visible >= width - 1)
            break;
        out += char;
        visible++;
    }
    return out + ellipsis;
}
function frame(lines, title, width = 76) {
    const innerWidth = width - 4;
    const topTitle = title ? ` ${title} ` : "";
    const topLine = title
        ? `╭${"─".repeat(Math.max(0, Math.floor((width - visibleLength(topTitle) - 2) / 2)))}${topTitle}${"─".repeat(Math.max(0, width - visibleLength(topTitle) - 2 - Math.floor((width - visibleLength(topTitle) - 2) / 2)))}╮`
        : `╭${"─".repeat(width - 2)}╮`;
    const body = lines.map((line) => `│ ${fit(line, innerWidth)} │`).join("\n");
    const bottom = `╰${"─".repeat(width - 2)}╯`;
    return [topLine, body, bottom].join("\n");
}
function center(text, width) {
    const length = visibleLength(text);
    if (length >= width)
        return text;
    const padding = Math.floor((width - length) / 2);
    return `${" ".repeat(padding)}${text}`;
}
function promptLine(question_1) {
    return __awaiter(this, arguments, void 0, function* (question, defaultValue = "") {
        const rl = (0, promises_1.createInterface)({ input: node_process_1.stdin, output: node_process_1.stdout });
        try {
            const suffix = defaultValue ? ` ${dim(`[${defaultValue}]`)}` : "";
            const answer = yield rl.question(`${question}${suffix} `);
            return (answer.trim() || defaultValue).trim();
        }
        finally {
            rl.close();
        }
    });
}
function promptOptionalLine(question_1) {
    return __awaiter(this, arguments, void 0, function* (question, defaultValue = "") {
        const rl = (0, promises_1.createInterface)({ input: node_process_1.stdin, output: node_process_1.stdout });
        try {
            const suffix = defaultValue ? ` ${dim(`[${defaultValue}]`)}` : "";
            const answer = yield rl.question(`${question}${suffix} `);
            return answer.trim();
        }
        finally {
            rl.close();
        }
    });
}
function promptNumber(question, defaultValue, min, max) {
    return __awaiter(this, void 0, void 0, function* () {
        while (true) {
            const raw = yield promptLine(question, String(defaultValue));
            const value = Number(raw);
            if (!Number.isFinite(value)) {
                console.log(yellow("Please enter a valid number."));
                continue;
            }
            if (min !== undefined && value < min) {
                console.log(yellow(`Value must be at least ${min}.`));
                continue;
            }
            if (max !== undefined && value > max) {
                console.log(yellow(`Value must be at most ${max}.`));
                continue;
            }
            return Math.round(value);
        }
    });
}
function promptConfirm(question, defaultValue) {
    return __awaiter(this, void 0, void 0, function* () {
        const suffix = defaultValue ? "Y/n" : "y/N";
        while (true) {
            const raw = yield promptLine(`${question} ${dim(`(${suffix})`)}`);
            if (!raw)
                return defaultValue;
            const normalized = raw.toLowerCase();
            if (["y", "yes", "true", "1"].includes(normalized))
                return true;
            if (["n", "no", "false", "0"].includes(normalized))
                return false;
            console.log(yellow("Please answer with y or n."));
        }
    });
}
function promptSecret(question) {
    return __awaiter(this, void 0, void 0, function* () {
        if (!node_process_1.stdin.isTTY || !node_process_1.stdout.isTTY) {
            throw new Error("Interactive setup requires a TTY.");
        }
        return new Promise((resolve, reject) => {
            let value = "";
            const cleanup = () => {
                node_process_1.stdin.off("data", onData);
                if (node_process_1.stdin.isTTY)
                    node_process_1.stdin.setRawMode(false);
                node_process_1.stdin.pause();
            };
            const finish = () => {
                cleanup();
                node_process_1.stdout.write("\n");
                resolve(value.trim());
            };
            const onData = (chunk) => {
                for (const char of chunk) {
                    if (char === "\u0003") {
                        cleanup();
                        reject(new Error("Setup cancelled."));
                        return;
                    }
                    if (char === "\r" || char === "\n") {
                        finish();
                        return;
                    }
                    if (char === "\u007f" || char === "\b") {
                        if (value.length > 0) {
                            value = value.slice(0, -1);
                            node_process_1.stdout.write("\b \b");
                        }
                        continue;
                    }
                    if (char === "\u001b")
                        continue;
                    value += char;
                    node_process_1.stdout.write("•");
                }
            };
            node_process_1.stdout.write(question + " ");
            node_process_1.stdin.setEncoding("utf8");
            node_process_1.stdin.setRawMode(true);
            node_process_1.stdin.resume();
            node_process_1.stdin.on("data", onData);
        });
    });
}
function validateDiscordToken(token) {
    return __awaiter(this, void 0, void 0, function* () {
        try {
            const response = yield fetch("https://discord.com/api/v9/users/@me", {
                headers: { Authorization: token },
            });
            return response.ok;
        }
        catch (_a) {
            return false;
        }
    });
}
function banner() {
    const width = 76;
    const title = bold(cyan("LyricsStatus")) + " " + dim("terminal setup");
    const subtitle = "Configure the application from the terminal.";
    const status = `${green("●")} ${dim("Token, display style, timing, updates")}`;
    return [
        frame([
            center(title, width - 4),
            center(dim(subtitle), width - 4),
            "",
            center(status, width - 4),
        ], "Welcome", width),
        ""
    ].join("\n");
}
function startupMenu() {
    return frame([
        `${bold(cyan("1"))}  Web panel setup`,
        `${bold(cyan("2"))}  Terminal setup`,
        "",
        dim("Choose the setup style for this launch."),
    ], `${bold("Choose setup mode")}`, 76);
}
function summary() {
    const advanced = Settings_1.Settings.view.advanced.enabled ? green("enabled") : dim("disabled");
    const web = dim("legacy web panel is optional via --web");
    return frame([
        `${cyan("Token:")} ${dim(Settings_1.Settings.credentials.token ? "saved" : "missing")}`,
        `${cyan("Timestamp:")} ${Settings_1.Settings.view.timestamp ? green("on") : dim("off")}`,
        `${cyan("Label:")} ${Settings_1.Settings.view.label ? green("on") : dim("off")}`,
        // Emoji is optional and intentionally empty by default.
        `${cyan("Emoji: ")} ${Settings_1.Settings.view.emoji ? green(Settings_1.Settings.view.emoji) : dim("none")}`,
        `${cyan("Advanced:")} ${advanced}`,
        `${cyan("Offset:")} ${yellow(`${Settings_1.Settings.timings.sendTimeOffset} ms`)}`,
        `${cyan("Autooffset:")} ${Settings_1.Settings.timings.enableAutooffset ? green(`${Settings_1.Settings.timings.autooffset} samples`) : dim("off")}`,
        `${cyan("Auto update:")} ${Settings_1.Settings.update.enableAutoupdate ? green("on") : dim("off")}`,
        `${cyan("Web panel:")} ${web}`,
    ], `${bold("Setup complete")}`, 76);
}
class Tui {
    static isDashboardSuspended() {
        return Tui._dashboardSuspended;
    }
    static setDashboardSuspended(v) {
        Tui._dashboardSuspended = v;
    }
    static attachDashboardListener() {
        if (Tui._dashboardListenerAttached)
            return;
        if (!node_process_1.stdin.isTTY || !node_process_1.stdout.isTTY)
            return;
        const onData = (chunk) => {
            if (Tui._dashboardSuspended)
                return;
            const buf = typeof chunk === "string" ? Buffer.from(chunk, "utf8") : chunk;
            for (const b of buf) {
                if (b === 0x03) {
                    try {
                        Tui.detachDashboardListener();
                    }
                    catch (_a) { }
                    try {
                        if (node_process_1.stdin.isTTY)
                            node_process_1.stdin.setRawMode(false);
                    }
                    catch (_b) { }
                    process.exit(0);
                }
                if (b === 0x13) {
                    ;
                    (() => __awaiter(this, void 0, void 0, function* () {
                        try {
                            Tui.setDashboardSuspended(true);
                            Tui.detachDashboardListener();
                            yield Tui.runUpdateSettingsWizard();
                        }
                        catch (_a) {
                        }
                        finally {
                            Tui.attachDashboardListener();
                            Tui.setDashboardSuspended(false);
                        }
                    }))();
                    continue;
                }
                if (b === 0x02) {
                    ;
                    (() => __awaiter(this, void 0, void 0, function* () {
                        try {
                            Tui.setDashboardSuspended(true);
                            Tui.detachDashboardListener();
                            if (Tui.onOpenWeb)
                                Tui.onOpenWeb();
                        }
                        catch (_a) {
                        }
                        finally {
                            Tui.attachDashboardListener();
                            Tui.setDashboardSuspended(false);
                        }
                    }))();
                    continue;
                }
            }
        };
        Tui._onData = onData;
        node_process_1.stdin.setEncoding("utf8");
        if (node_process_1.stdin.isTTY) {
            node_process_1.stdin.setRawMode(true);
            node_process_1.stdin.resume();
        }
        node_process_1.stdin.on("data", Tui._onData);
        Tui._dashboardListenerAttached = true;
    }
    static detachDashboardListener() {
        if (!Tui._dashboardListenerAttached)
            return;
        if (Tui._onData)
            node_process_1.stdin.off("data", Tui._onData);
        Tui._onData = undefined;
        if (node_process_1.stdin.isTTY) {
            try {
                node_process_1.stdin.setRawMode(false);
            }
            catch (_a) {
                // ignore if raw mode cannot be toggled
            }
        }
        Tui._dashboardListenerAttached = false;
    }
    static chooseStartupMode() {
        return __awaiter(this, void 0, void 0, function* () {
            if (!node_process_1.stdin.isTTY || !node_process_1.stdout.isTTY)
                return "web";
            clearScreen();
            console.log(banner());
            console.log(startupMenu());
            while (true) {
                const answer = (yield promptLine("Choose 1 for web or 2 for terminal", "2")).toLowerCase();
                if (["1", "web", "w"].includes(answer))
                    return "web";
                if (["2", "terminal", "t"].includes(answer))
                    return "terminal";
                console.log(yellow("Please choose web or terminal."));
            }
        });
    }
    static runTerminalFlow() {
        return __awaiter(this, void 0, void 0, function* () {
            if (!node_process_1.stdin.isTTY || !node_process_1.stdout.isTTY) {
                throw new Error("Terminal mode requires an interactive terminal.");
            }
            if (!Settings_1.Settings.credentials.token) {
                yield Tui.runSetupWizard();
                return;
            }
            clearScreen();
            console.log(banner());
            console.log(frame([
                `${bold(cyan("1"))}  Full setup`,
                `${bold(cyan("2"))}  Update settings`,
                `${bold(cyan("3"))}  Start now`,
                "",
                dim("Pick what you want to edit before the dashboard starts."),
            ], `${bold("Terminal mode")}`, 76));
            while (true) {
                const answer = (yield promptLine("Choose 1, 2, or 3", "3")).toLowerCase();
                if (["1", "full", "setup"].includes(answer)) {
                    yield Tui.runSetupWizard();
                    return;
                }
                if (["2", "update", "updates"].includes(answer)) {
                    yield Tui.runUpdateSettingsWizard();
                    return;
                }
                if (["3", "start", "now"].includes(answer))
                    return;
                console.log(yellow("Please choose 1, 2, or 3."));
            }
        });
    }
    static renderRuntimeDashboard(state) {
        var _a;
        Tui.attachDashboardListener();
        const song = state.songName || "—";
        const artist = state.songAuthor || "—";
        const elapsed = Tui.formatSeconds(Math.floor(state.songProgress / 1000));
        const lyrics = ((_a = state.currentLine) === null || _a === void 0 ? void 0 : _a.text) || "Waiting for lyrics...";
        const source = state.lastFetchedFrom || "—";
        // Keep the dashboard compact while still showing the active emoji when configured.
        const emoji = Settings_1.Settings.view.emoji ? `${Settings_1.Settings.view.emoji} ` : "";
        const lines = [];
        lines.push(`${bold(cyan("Song"))}   ${dim("│")} ${emoji}${song}`);
        lines.push(`${bold(cyan("Artist"))} ${dim("│")} ${artist}`);
        if (Settings_1.Settings.view.timestamp) {
            lines.push(`${bold(cyan("Time"))}   ${dim("│")} ${yellow(elapsed)}`);
        }
        lines.push(`${bold(cyan("Ping"))}   ${dim("│")} ${yellow(state.latency !== undefined ? `${state.latency} ms` : "—")}`);
        // The lyrics row stays visible so the current text remains readable in the preview.
        lines.push(`${bold(cyan("Lyrics"))} ${dim("│")} ${green(lyrics)}`);
        lines.push(`${bold(cyan("Source"))} ${dim("│")} ${magenta(source)}`);
        lines.push("");
        lines.push(`${dim("Tip: ")}Press ${bold("Ctrl+S")} to open settings, ${bold("Ctrl+B")} to open the web panel.`);
        clearScreen();
        console.log(frame(lines, `${bold(cyan("LyricsStatus"))}`, 84));
    }
    static runSetupWizard() {
        return __awaiter(this, void 0, void 0, function* () {
            if (!node_process_1.stdin.isTTY || !node_process_1.stdout.isTTY) {
                throw new Error("The setup wizard requires an interactive terminal.");
            }
            clearScreen();
            console.log(banner());
            console.log(frame([
                `${dim("Step 1")} ${bold("Discord token")}`,
                "This token is stored locally in settings.json and is only used to update your custom status.",
            ], "Account", 76));
            const existingToken = Settings_1.Settings.credentials.token;
            let token = existingToken;
            if (!token || !(yield promptConfirm("Reuse the saved Discord token?", true))) {
                token = yield promptSecret(yellow("Enter your Discord token:"));
            }
            while (!(yield validateDiscordToken(token))) {
                console.log(yellow("That token did not validate. Try again."));
                token = yield promptSecret(yellow("Enter your Discord token:"));
            }
            Settings_1.Settings.credentials.token = token;
            console.log("");
            console.log(frame([
                `${dim("Step 2")} ${bold("Status style")}`,
                "Choose how the song text appears in Discord.",
            ], "Preview", 76));
            Settings_1.Settings.view.timestamp = yield promptConfirm("Show playback timestamp?", Settings_1.Settings.view.timestamp);
            Settings_1.Settings.view.label = yield promptConfirm("Show the Song lyrics label?", Settings_1.Settings.view.label);
            // Optional emoji to show next to the song/status. Empty = none.
            Settings_1.Settings.view.emoji = yield promptOptionalLine("Show an emoji?", Settings_1.Settings.view.emoji);
            const enableAdvanced = yield promptConfirm("Enable advanced custom status template?", Settings_1.Settings.view.advanced.enabled);
            Settings_1.Settings.view.advanced.enabled = enableAdvanced;
            if (enableAdvanced) {
                Settings_1.Settings.view.advanced.customEmoji = yield promptOptionalLine("Custom emoji", Settings_1.Settings.view.advanced.customEmoji);
                Settings_1.Settings.view.advanced.customStatus = yield promptLine("Custom status template", Settings_1.Settings.view.advanced.customStatus);
            }
            console.log("");
            console.log(frame([
                `${dim("Step 3")} ${bold("Timing")}`,
                "Fine-tune how early the status changes relative to the lyric timestamp.",
            ], "Sync", 76));
            Settings_1.Settings.timings.sendTimeOffset = yield promptNumber("Send time offset (ms)", Settings_1.Settings.timings.sendTimeOffset, 0, 10000);
            Settings_1.Settings.timings.enableAutooffset = yield promptConfirm("Enable autooffset?", Settings_1.Settings.timings.enableAutooffset);
            Settings_1.Settings.timings.autooffset = yield promptNumber("Autooffset samples", Settings_1.Settings.timings.autooffset, 1, 20);
            console.log("");
            console.log(frame([
                `${dim("Step 4")} ${bold("Updates")}`,
                "Optional update checks keep the beta build aligned with the latest fixes.",
            ], "Maintenance", 76));
            Settings_1.Settings.update.enableAutoupdate = yield promptConfirm("Enable automatic update checks?", Settings_1.Settings.update.enableAutoupdate);
            Settings_1.Settings.save();
            console.log("");
            console.log(summary());
            console.log("");
            console.log(dim("Press Enter to continue, or Ctrl+C to exit."));
            yield promptLine("");
        });
    }
    static runUpdateSettingsWizard() {
        return __awaiter(this, void 0, void 0, function* () {
            if (!node_process_1.stdin.isTTY || !node_process_1.stdout.isTTY) {
                throw new Error("The settings editor requires an interactive terminal.");
            }
            while (true) {
                clearScreen();
                console.log(banner());
                console.log(frame([
                    `${bold(cyan("1"))}  Account (Discord token)`,
                    `${bold(cyan("2"))}  View (timestamp, label, advanced)`,
                    `${bold(cyan("3"))}  Timing (offset, autooffset)`,
                    `${bold(cyan("4"))}  Updates (auto-update)`,
                    "",
                    dim("Edit the settings you configured during startup."),
                ], `${bold("Settings editor")}`, 76));
                console.log(frame([
                    `${cyan("Current summary:")}`,
                    `${cyan("Token: ")} ${dim(Settings_1.Settings.credentials.token ? "saved" : "missing")}`,
                    `${cyan("Timestamp: ")} ${Settings_1.Settings.view.timestamp ? green("on") : dim("off")}`,
                    `${cyan("Label: ")} ${Settings_1.Settings.view.label ? green("on") : dim("off")}`,
                    `${cyan("Emoji: ")} ${Settings_1.Settings.view.emoji ? green(Settings_1.Settings.view.emoji) : dim("none")}`,
                    `${cyan("Auto update: ")} ${Settings_1.Settings.update.enableAutoupdate ? green("on") : dim("off")}`,
                ], `${bold("Summary")}`, 76));
                console.log(dim("Choose a section to edit, or press Enter to finish."));
                const choice = (yield promptLine("Choose 1-4 or Enter to save/exit", "")).trim();
                if (!choice) {
                    Settings_1.Settings.save();
                    clearScreen();
                    console.log(frame([
                        "Settings saved.",
                    ], `${bold("Saved")}`, 76));
                    console.log("");
                    console.log(dim("Press Enter to continue, or Ctrl+C to exit."));
                    yield promptLine("");
                    return;
                }
                if (["1", "account"].includes(choice.toLowerCase())) {
                    // Account: change/validate token
                    clearScreen();
                    console.log(banner());
                    console.log(frame([
                        `${dim("Account")}`,
                        "Change the Discord token used to update your custom status.",
                    ], "Account", 76));
                    const reuse = yield promptConfirm("Reuse the saved Discord token?", !!Settings_1.Settings.credentials.token);
                    let token = Settings_1.Settings.credentials.token;
                    if (!reuse) {
                        token = yield promptSecret(yellow("Enter your Discord token:"));
                        while (!(yield validateDiscordToken(token))) {
                            console.log(yellow("That token did not validate. Try again or press Ctrl+C to cancel."));
                            token = yield promptSecret(yellow("Enter your Discord token:"));
                        }
                    }
                    Settings_1.Settings.credentials.token = token;
                    Settings_1.Settings.save();
                    console.log("");
                    console.log(frame([`${cyan("Token saved.")}`], `${bold("Saved")}`, 76));
                    console.log("");
                    console.log(dim("Press Enter to continue."));
                    yield promptLine("");
                    continue;
                }
                if (["2", "view"].includes(choice.toLowerCase())) {
                    clearScreen();
                    console.log(banner());
                    console.log(frame([
                        `${dim("View settings")}`,
                        "Toggle how status text is composed.",
                    ], "View", 76));
                    Settings_1.Settings.view.timestamp = yield promptConfirm("Show playback timestamp?", Settings_1.Settings.view.timestamp);
                    Settings_1.Settings.view.label = yield promptConfirm("Show the Song lyrics label?", Settings_1.Settings.view.label);
                    Settings_1.Settings.view.emoji = yield promptOptionalLine("Show an emoji?", Settings_1.Settings.view.emoji);
                    const enableAdvanced = yield promptConfirm("Enable advanced custom status template?", Settings_1.Settings.view.advanced.enabled);
                    Settings_1.Settings.view.advanced.enabled = enableAdvanced;
                    if (enableAdvanced) {
                        Settings_1.Settings.view.advanced.customEmoji = yield promptOptionalLine("Custom emoji", Settings_1.Settings.view.advanced.customEmoji);
                        Settings_1.Settings.view.advanced.customStatus = yield promptLine("Custom status template", Settings_1.Settings.view.advanced.customStatus);
                    }
                    Settings_1.Settings.save();
                    console.log("");
                    console.log(frame([`${cyan("View settings saved.")}`], `${bold("Saved")}`, 76));
                    console.log("");
                    console.log(dim("Press Enter to continue."));
                    yield promptLine("");
                    continue;
                }
                if (["3", "timing"].includes(choice.toLowerCase())) {
                    clearScreen();
                    console.log(banner());
                    console.log(frame([
                        `${dim("Timing")}`,
                        "Configure status send offsets and autooffset behavior.",
                    ], "Timing", 76));
                    Settings_1.Settings.timings.sendTimeOffset = yield promptNumber("Send time offset (ms)", Settings_1.Settings.timings.sendTimeOffset, 0, 10000);
                    Settings_1.Settings.timings.enableAutooffset = yield promptConfirm("Enable autooffset?", Settings_1.Settings.timings.enableAutooffset);
                    Settings_1.Settings.timings.autooffset = yield promptNumber("Autooffset samples", Settings_1.Settings.timings.autooffset, 1, 20);
                    Settings_1.Settings.save();
                    console.log("");
                    console.log(frame([`${cyan("Timing saved.")}`], `${bold("Saved")}`, 76));
                    console.log("");
                    console.log(dim("Press Enter to continue."));
                    yield promptLine("");
                    continue;
                }
                if (["4", "update"].includes(choice.toLowerCase())) {
                    clearScreen();
                    console.log(banner());
                    console.log(frame([
                        `${dim("Updates")}`,
                        "Automatic update checks and maintenance options.",
                    ], "Updates", 76));
                    Settings_1.Settings.update.enableAutoupdate = yield promptConfirm("Enable automatic update checks?", Settings_1.Settings.update.enableAutoupdate);
                    Settings_1.Settings.save();
                    console.log("");
                    console.log(frame([`${cyan("Update settings saved.")}`], `${bold("Saved")}`, 76));
                    console.log("");
                    console.log(dim("Press Enter to continue."));
                    yield promptLine("");
                    continue;
                }
                console.log(yellow("Unknown choice — pick 1-4 or press Enter to finish."));
                yield promptLine("");
            }
        });
    }
    static formatSeconds(totalSeconds) {
        const minutes = Math.floor(totalSeconds / 60);
        const seconds = totalSeconds % 60;
        return `${minutes}:${seconds.toString().padStart(2, "0")}`;
    }
}
exports.Tui = Tui;
Tui._dashboardListenerAttached = false;
Tui._dashboardSuspended = false;
/** Callback invoked when user presses Ctrl+W in the dashboard. */
Tui.onOpenWeb = null;
