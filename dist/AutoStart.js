"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.AutoStart = void 0;
const node_fs_1 = require("node:fs");
const node_os_1 = require("node:os");
const node_path_1 = __importDefault(require("node:path"));
const node_child_process_1 = require("node:child_process");
/**
 * Lightweight cross-platform autostart helper.
 * - Linux: creates a .desktop file in ~/.config/autostart
 * - macOS: creates a LaunchAgent plist in ~/Library/LaunchAgents
 * - Windows: sets HKCU Run registry value
 */
class AutoStart {
    static enable(enable) {
        try {
            const plt = (0, node_os_1.platform)();
            if (plt === "linux")
                return this.enableLinux(enable);
            if (plt === "darwin")
                return this.enableMac(enable);
            if (plt === "win32")
                return this.enableWindows(enable);
        }
        catch (e) {
            // best-effort; do not crash the app
            // eslint-disable-next-line no-console
            console.error("AutoStart error:", e.message);
        }
    }
    static nodeCommand() {
        // Return a quoted absolute command so autostart works regardless of cwd.
        const node = process.execPath;
        const script = process.argv[1] ? node_path_1.default.resolve(process.argv[1]) : "";
        if (!script)
            return `"${node}"`;
        return `"${node}" "${script}"`;
    }
    static enableLinux(enable) {
        const configDir = node_path_1.default.join((0, node_os_1.homedir)(), ".config", "autostart");
        if (!(0, node_fs_1.existsSync)(configDir))
            (0, node_fs_1.mkdirSync)(configDir, { recursive: true });
        const desktopPath = node_path_1.default.join(configDir, `${this.appName}.desktop`);
        if (!enable) {
            if ((0, node_fs_1.existsSync)(desktopPath))
                (0, node_fs_1.unlinkSync)(desktopPath);
            return;
        }
        const exec = this.nodeCommand();
        const content = [
            "[Desktop Entry]",
            `Type=Application`,
            `Name=${this.appName}`,
            `Exec=${exec}`,
            `X-GNOME-Autostart-enabled=true`,
        ].join("\n");
        (0, node_fs_1.writeFileSync)(desktopPath, content, { encoding: "utf8" });
    }
    static enableMac(enable) {
        const agents = node_path_1.default.join((0, node_os_1.homedir)(), "Library", "LaunchAgents");
        if (!(0, node_fs_1.existsSync)(agents))
            (0, node_fs_1.mkdirSync)(agents, { recursive: true });
        const label = `com.${this.appName}.autostart`;
        const plistPath = node_path_1.default.join(agents, `${label}.plist`);
        if (!enable) {
            if ((0, node_fs_1.existsSync)(plistPath))
                (0, node_fs_1.unlinkSync)(plistPath);
            try {
                (0, node_child_process_1.execSync)(`launchctl unload ${plistPath}`);
            }
            catch (_a) { }
            return;
        }
        const exec = this.nodeCommand();
        const plist = `<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n<plist version="1.0">\n<dict>\n  <key>Label</key>\n  <string>${label}</string>\n  <key>ProgramArguments</key>\n  <array>\n    <string>${process.execPath}</string>\n    <string>${process.argv[1] || ""}</string>\n  </array>\n  <key>RunAtLoad</key>\n  <true/>\n</dict>\n</plist>`;
        (0, node_fs_1.writeFileSync)(plistPath, plist, { encoding: "utf8" });
        try {
            (0, node_child_process_1.execSync)(`launchctl load ${plistPath}`);
        }
        catch (_b) { }
    }
    static enableWindows(enable) {
        // Use HKCU Run to avoid needing admin rights.
        const name = "LyricsStatus";
        const cmd = this.nodeCommand();
        if (!enable) {
            try {
                (0, node_child_process_1.execSync)(`reg delete HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run /v ${name} /f`);
            }
            catch (_a) {
                // ignore
            }
            return;
        }
        try {
            (0, node_child_process_1.execSync)(`reg add HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run /v ${name} /t REG_SZ /d "${cmd}" /f`);
        }
        catch (e) {
            // ignore
        }
    }
}
exports.AutoStart = AutoStart;
AutoStart.appName = "lyrics-status";
exports.default = AutoStart;
