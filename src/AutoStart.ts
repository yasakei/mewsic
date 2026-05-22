import { writeFileSync, unlinkSync, existsSync, mkdirSync } from "node:fs"
import { homedir, platform } from "node:os"
import path from "node:path"
import { execSync } from "node:child_process"

/**
 * Lightweight cross-platform autostart helper.
 * - Linux: creates a .desktop file in ~/.config/autostart
 * - macOS: creates a LaunchAgent plist in ~/Library/LaunchAgents
 * - Windows: sets HKCU Run registry value
 */
export class AutoStart {
    private static appName = "lyrics-status"

    public static enable(enable: boolean): void {
        try {
            const plt = platform()
            if (plt === "linux") return this.enableLinux(enable)
            if (plt === "darwin") return this.enableMac(enable)
            if (plt === "win32") return this.enableWindows(enable)
        } catch (e) {
            // best-effort; do not crash the app
            // eslint-disable-next-line no-console
            console.error("AutoStart error:", (e as Error).message)
        }
    }

    private static nodeCommand(): string {
        // Return a quoted absolute command so autostart works regardless of cwd.
        const node = process.execPath
        const script = process.argv[1] ? path.resolve(process.argv[1]) : ""
        if (!script) return `"${node}"`
        return `"${node}" "${script}"`
    }

    private static enableLinux(enable: boolean): void {
        const configDir = path.join(homedir(), ".config", "autostart")
        if (!existsSync(configDir)) mkdirSync(configDir, { recursive: true })

        const desktopPath = path.join(configDir, `${this.appName}.desktop`)

        if (!enable) {
            if (existsSync(desktopPath)) unlinkSync(desktopPath)
            return
        }

        const exec = this.nodeCommand()
        const content = [
            "[Desktop Entry]",
            `Type=Application`,
            `Name=${this.appName}`,
            `Exec=${exec}`,
            `X-GNOME-Autostart-enabled=true`,
        ].join("\n")

        writeFileSync(desktopPath, content, { encoding: "utf8" })
    }

    private static enableMac(enable: boolean): void {
        const agents = path.join(homedir(), "Library", "LaunchAgents")
        if (!existsSync(agents)) mkdirSync(agents, { recursive: true })

        const label = `com.${this.appName}.autostart`
        const plistPath = path.join(agents, `${label}.plist`)

        if (!enable) {
            if (existsSync(plistPath)) unlinkSync(plistPath)
            try { execSync(`launchctl unload ${plistPath}`) } catch {}
            return
        }

        const exec = this.nodeCommand()
        const plist = `<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n<plist version="1.0">\n<dict>\n  <key>Label</key>\n  <string>${label}</string>\n  <key>ProgramArguments</key>\n  <array>\n    <string>${process.execPath}</string>\n    <string>${process.argv[1] || ""}</string>\n  </array>\n  <key>RunAtLoad</key>\n  <true/>\n</dict>\n</plist>`

        writeFileSync(plistPath, plist, { encoding: "utf8" })
        try { execSync(`launchctl load ${plistPath}`) } catch {}
    }

    private static enableWindows(enable: boolean): void {
        // Use HKCU Run to avoid needing admin rights.
        const name = "LyricsStatus"
        const cmd = this.nodeCommand()

        if (!enable) {
            try {
                execSync(`reg delete HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run /v ${name} /f`)
            } catch {
                // ignore
            }
            return
        }

        try {
            execSync(`reg add HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run /v ${name} /t REG_SZ /d "${cmd}" /f`)
        } catch (e) {
            // ignore
        }
    }
}

export default AutoStart
