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
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const LyricsFetcher_1 = require("./LyricsFetcher");
const LrcLibSource_1 = require("./Sources/LrcLibSource");
const NetEaseMusicSource_1 = require("./Sources/NetEaseMusicSource");
const QQMusicSource_1 = require("./Sources/QQMusicSource");
const PlaybackStateUpdater_1 = require("./PlaybackStateUpdater");
const PlaybackState_1 = require("./PlaybackState");
const StatusChanger_1 = require("./StatusChanger");
const Debug_1 = require("./Debug");
const Tui_1 = require("./Tui");
const Server_1 = require("./Panel/Server");
const Settings_1 = require("./Settings");
const Updater_1 = require("./Updater");
const uuid_1 = require("uuid");
const node_fs_1 = require("node:fs");
const node_os_1 = require("node:os");
const node_path_1 = __importDefault(require("node:path"));
// ─── Bootstrap ────────────────────────────────────────────────────────────────
const args = new Set(process.argv.slice(2));
const forceSetup = args.has("--setup");
const settingsOnly = args.has("--settings");
const useWebPanel = args.has("--web") || process.env.LYRICS_STATUS_WEB === "1";
function buildRuntimeSnapshot(playbackState, lyricsFetcher) {
    var _a, _b, _c, _d, _e, _f;
    // Keep the live dashboard useful even before the current line has been set.
    const previewLine = (_e = (_b = (_a = playbackState.currentLine) === null || _a === void 0 ? void 0 : _a.text) !== null && _b !== void 0 ? _b : (_d = (_c = playbackState.lyrics) === null || _c === void 0 ? void 0 : _c.lines.find((line) => line.time >= playbackState.songProgress)) === null || _d === void 0 ? void 0 : _d.text) !== null && _e !== void 0 ? _e : null;
    return {
        songName: playbackState.songName,
        songAuthor: playbackState.songAuthor,
        songProgress: playbackState.songProgress,
        currentLine: (_f = playbackState.currentLine) !== null && _f !== void 0 ? _f : (previewLine ? { text: previewLine } : null),
        lastFetchedFrom: lyricsFetcher.lastFetchedFrom,
        latency: StatusChanger_1.StatusChanger.lastLatency,
    };
}
function bootstrap() {
    return __awaiter(this, void 0, void 0, function* () {
        Settings_1.Settings.load();
        if (settingsOnly) {
            yield Tui_1.Tui.runUpdateSettingsWizard();
            return;
        }
        if (forceSetup || !Settings_1.Settings.credentials.token) {
            const chosenMode = forceSetup ? "terminal" : yield Tui_1.Tui.chooseStartupMode();
            if (chosenMode === "web") {
                if (forceSetup || !Settings_1.Settings.credentials.token) {
                    console.log("Open the web panel at http://localhost:8999 and finish setup there.");
                }
                (0, Server_1.startServer)();
                return;
            }
            yield Tui_1.Tui.runTerminalFlow();
            if (forceSetup)
                return;
        }
        if (Settings_1.Settings.update.enableAutoupdate) {
            try {
                yield Updater_1.Updater.tryUpdate();
            }
            catch (e) {
                Debug_1.Debug.write(`Auto-update failed: ${e.stack}`);
            }
        }
        init(useWebPanel);
    });
}
Tui_1.Tui.onOpenWeb = () => {
    try {
        (0, Server_1.startServer)();
        Debug_1.Debug.write("Web panel started from dashboard (keybind)");
        try {
            const { exec } = require("child_process");
            const url = "http://localhost:8999";
            if (process.platform === "win32") {
                exec(`start "" "${url}"`);
            }
            else if (process.platform === "darwin") {
                exec(`open "${url}"`);
            }
            else {
                exec(`xdg-open "${url}"`);
            }
        }
        catch (e) {
            Debug_1.Debug.write(`Failed to launch browser: ${e.stack}`);
        }
    }
    catch (e) {
        Debug_1.Debug.write(`Failed to start web panel: ${e.stack}`);
    }
};
bootstrap().catch((e) => {
    var _a;
    Debug_1.Debug.write((_a = e.stack) !== null && _a !== void 0 ? _a : e.message);
    process.exit(1);
});
// ─── Init ─────────────────────────────────────────────────────────────────────
function init(enableWebPanel) {
    // Write PID file so external scripts can detect a running instance.
    try {
        const cfg = node_path_1.default.join((0, node_os_1.homedir)(), ".config", "lyrics-status");
        if (!(0, node_fs_1.existsSync)(cfg))
            (0, node_fs_1.mkdirSync)(cfg, { recursive: true });
        const pidFile = node_path_1.default.join(cfg, "lyrics-status.pid");
        (0, node_fs_1.writeFileSync)(pidFile, String(process.pid), { encoding: "utf8" });
        process.on("exit", () => { try {
            (0, node_fs_1.unlinkSync)(pidFile);
        }
        catch (_a) { } });
        process.on("SIGINT", () => process.exit(0));
        process.on("SIGTERM", () => process.exit(0));
    }
    catch (_a) {
        // best-effort
    }
    if (!Settings_1.Settings.credentials.uuid) {
        Settings_1.Settings.credentials.uuid = (0, uuid_1.v4)();
        Settings_1.Settings.save();
    }
    const lyricsFetcher = new LyricsFetcher_1.LyricsFetcher();
    lyricsFetcher.addSource(new LrcLibSource_1.LrcLibSource());
    lyricsFetcher.addSource(new NetEaseMusicSource_1.NetEaseMusicSource());
    lyricsFetcher.addSource(new QQMusicSource_1.QQMusicSource());
    const playbackState = new PlaybackState_1.PlaybackState();
    const playbackStateUpdater = new PlaybackStateUpdater_1.PlaybackStateUpdater(playbackState, lyricsFetcher);
    const statusChanger = new StatusChanger_1.StatusChanger(playbackState);
    setInterval(() => playbackStateUpdater.update(), 2000);
    let lastTick = Date.now();
    let lastRender = 0;
    setInterval(() => {
        const now = Date.now();
        const delta = now - lastTick;
        lastTick = now;
        playbackState.songProgress += delta;
        statusChanger.changeStatus();
        if (playbackState.ended)
            statusChanger.songChanged();
        if (now - lastRender >= 250) {
            lastRender = now;
            if (Tui_1.Tui.isDashboardSuspended())
                return;
            Tui_1.Tui.renderRuntimeDashboard(buildRuntimeSnapshot(playbackState, lyricsFetcher));
        }
    }, 1000 / 60);
    if (enableWebPanel)
        (0, Server_1.startServer)();
}
// ─── Error handling ───────────────────────────────────────────────────────────
process.on("uncaughtException", (e) => {
    var _a;
    Debug_1.Debug.write(`${e.stack}\n${(_a = e.cause) !== null && _a !== void 0 ? _a : ""}`);
    // Network errors are transient — keep running
    if (!e.message.includes("fetch failed"))
        process.exit(1);
});
