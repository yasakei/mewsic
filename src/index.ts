import { LyricsFetcher } from "./LyricsFetcher"
import { LrcLibSource } from "./Sources/LrcLibSource"
import { NetEaseMusicSource } from "./Sources/NetEaseMusicSource"
import { QQMusicSource } from "./Sources/QQMusicSource"
import { PlaybackStateUpdater } from "./PlaybackStateUpdater"
import { PlaybackState } from "./PlaybackState"
import { StatusChanger } from "./StatusChanger"
import { Debug } from "./Debug"
import { Tui } from "./Tui"
import { startServer } from "./Panel/Server"
import { Settings } from "./Settings"
import { Updater } from "./Updater"
import { v4 as uuidv4 } from "uuid"
import { writeFileSync, unlinkSync, existsSync, mkdirSync } from "node:fs"
import { homedir } from "node:os"
import path from "node:path"

// ─── Bootstrap ────────────────────────────────────────────────────────────────

const args = new Set(process.argv.slice(2))
const forceSetup = args.has("--setup")
const settingsOnly = args.has("--settings")
const useWebPanel = args.has("--web") || process.env.LYRICS_STATUS_WEB === "1"

function buildRuntimeSnapshot(
    playbackState: PlaybackState,
    lyricsFetcher: LyricsFetcher
): Parameters<typeof Tui.renderRuntimeDashboard>[0] {
    // Keep the live dashboard useful even before the current line has been set.
    const previewLine =
        playbackState.currentLine?.text ??
        playbackState.lyrics?.lines.find((line) => line.time >= playbackState.songProgress)?.text ??
        null

    return {
        songName: playbackState.songName,
        songAuthor: playbackState.songAuthor,
        songProgress: playbackState.songProgress,
        currentLine: playbackState.currentLine ?? (previewLine ? { text: previewLine } : null),
        lastFetchedFrom: lyricsFetcher.lastFetchedFrom,
        latency: StatusChanger.lastLatency,
    }
}

async function bootstrap(): Promise<void> {
    Settings.load()

    if (settingsOnly) {
        await Tui.runUpdateSettingsWizard()
        return
    }

    if (forceSetup || !Settings.credentials.token) {
        const chosenMode = forceSetup ? "terminal" : await Tui.chooseStartupMode()

        if (chosenMode === "web") {
            if (forceSetup || !Settings.credentials.token) {
                console.log("Open the web panel at http://localhost:8999 and finish setup there.")
            }

            startServer()
            return
        }

        await Tui.runTerminalFlow()

        if (forceSetup) return
    }

    if (Settings.update.enableAutoupdate) {
        try {
            await Updater.tryUpdate()
        } catch (e) {
            Debug.write(`Auto-update failed: ${(e as Error).stack}`)
        }
    }

    init(useWebPanel)
}

Tui.onOpenWeb = () => {
    try {
        startServer()
        Debug.write("Web panel started from dashboard (keybind)")

        try {
            const { exec } = require("child_process")
            const url = "http://localhost:8999"

            if (process.platform === "win32") {
                exec(`start "" "${url}"`)
            } else if (process.platform === "darwin") {
                exec(`open "${url}"`)
            } else {
                exec(`xdg-open "${url}"`)
            }
        } catch (e) {
            Debug.write(`Failed to launch browser: ${(e as Error).stack}`)
        }
    } catch (e) {
        Debug.write(`Failed to start web panel: ${(e as Error).stack}`)
    }
}

bootstrap().catch((e: Error) => {
    Debug.write(e.stack ?? e.message)
    process.exit(1)
})

// ─── Init ─────────────────────────────────────────────────────────────────────

function init(enableWebPanel: boolean): void {
    // Write PID file so external scripts can detect a running instance.
    try {
        const cfg = path.join(homedir(), ".config", "lyrics-status")
        if (!existsSync(cfg)) mkdirSync(cfg, { recursive: true })
        const pidFile = path.join(cfg, "lyrics-status.pid")
        writeFileSync(pidFile, String(process.pid), { encoding: "utf8" })
        process.on("exit", () => { try { unlinkSync(pidFile) } catch {} })
        process.on("SIGINT", () => process.exit(0))
        process.on("SIGTERM", () => process.exit(0))
    } catch {
        // best-effort
    }
    if (!Settings.credentials.uuid) {
        Settings.credentials.uuid = uuidv4()
        Settings.save()
    }

    const lyricsFetcher = new LyricsFetcher()
    lyricsFetcher.addSource(new LrcLibSource())
    lyricsFetcher.addSource(new NetEaseMusicSource())
    lyricsFetcher.addSource(new QQMusicSource())

    const playbackState        = new PlaybackState()
    const playbackStateUpdater = new PlaybackStateUpdater(playbackState, lyricsFetcher)
    const statusChanger        = new StatusChanger(playbackState)

    setInterval(() => playbackStateUpdater.update(), 2000)

    let lastTick = Date.now()
    let lastRender = 0

    setInterval(() => {
        const now   = Date.now()
        const delta = now - lastTick
        lastTick    = now

        playbackState.songProgress += delta

        statusChanger.changeStatus()

        if (playbackState.ended) statusChanger.songChanged()

        if (now - lastRender >= 250) {
            lastRender = now

            if (Tui.isDashboardSuspended()) return

            Tui.renderRuntimeDashboard(buildRuntimeSnapshot(playbackState, lyricsFetcher))
        }
    }, 1000 / 60)

    if (enableWebPanel) startServer()
}

// ─── Error handling ───────────────────────────────────────────────────────────

process.on("uncaughtException", (e: Error) => {
    Debug.write(`${e.stack}\n${e.cause ?? ""}`)

    // Network errors are transient — keep running
    if (!e.message.includes("fetch failed")) process.exit(1)
})
