import { createInterface } from "node:readline/promises"
import { stdin, stdout } from "node:process"
import { Settings } from "./Settings"

interface RuntimeSnapshot {
    songName: string | undefined
    songAuthor: string | undefined
    songProgress: number
    currentLine: { text: string } | null | undefined
    lastFetchedFrom: string
    latency?: number
}

export type StartupMode = "web" | "terminal"

const supportsAnsi = stdout.isTTY && !process.env.NO_COLOR

function color(code: number, text: string): string {
    if (!supportsAnsi) return text

    return `\u001b[${code}m${text}\u001b[0m`
}

function dim(text: string): string {
    return color(90, text)
}

function cyan(text: string): string {
    return color(96, text)
}

function green(text: string): string {
    return color(92, text)
}

function yellow(text: string): string {
    return color(93, text)
}

function magenta(text: string): string {
    return color(95, text)
}

function bold(text: string): string {
    return supportsAnsi ? `\u001b[1m${text}\u001b[0m` : text
}

function clearScreen(): void {
    if (!stdout.isTTY) {
        console.clear()
        return
    }

    stdout.write("\u001b[2J\u001b[H")
}

function stripAnsi(text: string): string {
    return text.replace(/\u001b\[[0-9;]*m/g, "")
}

function visibleLength(text: string): number {
    return stripAnsi(text).length
}

function fit(text: string, width: number): string {
    const plainLength = visibleLength(text)

    if (plainLength <= width) return text + " ".repeat(Math.max(0, width - plainLength))

    const ellipsis = "…"
    let visible = 0
    let out = ""

    for (let i = 0; i < text.length; i++) {
        const char = text[i]
        if (char === "\u001b") {
            const match = text.slice(i).match(/^\u001b\[[0-9;]*m/)
            if (match) {
                out += match[0]
                i += match[0].length - 1
                continue
            }
        }

        if (visible >= width - 1) break
        out += char
        visible++
    }

    return out + ellipsis
}

function frame(lines: string[], title?: string, width = 76): string {
    const innerWidth = width - 4
    const topTitle = title ? ` ${title} ` : ""
    const topLine = title
        ? `╭${"─".repeat(Math.max(0, Math.floor((width - visibleLength(topTitle) - 2) / 2)))}${topTitle}${"─".repeat(Math.max(0, width - visibleLength(topTitle) - 2 - Math.floor((width - visibleLength(topTitle) - 2) / 2)))}╮`
        : `╭${"─".repeat(width - 2)}╮`

    const body = lines.map((line) => `│ ${fit(line, innerWidth)} │`).join("\n")
    const bottom = `╰${"─".repeat(width - 2)}╯`

    return [topLine, body, bottom].join("\n")
}

function center(text: string, width: number): string {
    const length = visibleLength(text)
    if (length >= width) return text

    const padding = Math.floor((width - length) / 2)
    return `${" ".repeat(padding)}${text}`
}

async function promptLine(question: string, defaultValue = ""): Promise<string> {
    const rl = createInterface({ input: stdin, output: stdout })

    try {
        const suffix = defaultValue ? ` ${dim(`[${defaultValue}]`)}` : ""
        const answer = await rl.question(`${question}${suffix} `)
        return (answer.trim() || defaultValue).trim()
    } finally {
        rl.close()
    }
}

async function promptOptionalLine(question: string, defaultValue = ""): Promise<string> {
    const rl = createInterface({ input: stdin, output: stdout })

    try {
        const suffix = defaultValue ? ` ${dim(`[${defaultValue}]`)}` : ""
        const answer = await rl.question(`${question}${suffix} `)
        return answer.trim()
    } finally {
        rl.close()
    }
}

async function promptNumber(question: string, defaultValue: number, min?: number, max?: number): Promise<number> {
    while (true) {
        const raw = await promptLine(question, String(defaultValue))
        const value = Number(raw)

        if (!Number.isFinite(value)) {
            console.log(yellow("Please enter a valid number."))
            continue
        }

        if (min !== undefined && value < min) {
            console.log(yellow(`Value must be at least ${min}.`))
            continue
        }

        if (max !== undefined && value > max) {
            console.log(yellow(`Value must be at most ${max}.`))
            continue
        }

        return Math.round(value)
    }
}

async function promptConfirm(question: string, defaultValue: boolean): Promise<boolean> {
    const suffix = defaultValue ? "Y/n" : "y/N"

    while (true) {
        const raw = await promptLine(`${question} ${dim(`(${suffix})`)}`)

        if (!raw) return defaultValue

        const normalized = raw.toLowerCase()
        if (["y", "yes", "true", "1"].includes(normalized)) return true
        if (["n", "no", "false", "0"].includes(normalized)) return false

        console.log(yellow("Please answer with y or n."))
    }
}

async function promptSecret(question: string): Promise<string> {
    if (!stdin.isTTY || !stdout.isTTY) {
        throw new Error("Interactive setup requires a TTY.")
    }

    return new Promise<string>((resolve, reject) => {
        let value = ""

        const cleanup = (): void => {
            stdin.off("data", onData)
            if (stdin.isTTY) stdin.setRawMode(false)
            stdin.pause()
        }

        const finish = (): void => {
            cleanup()
            stdout.write("\n")
            resolve(value.trim())
        }

        const onData = (chunk: string): void => {
            for (const char of chunk) {
                if (char === "\u0003") {
                    cleanup()
                    reject(new Error("Setup cancelled."))
                    return
                }

                if (char === "\r" || char === "\n") {
                    finish()
                    return
                }

                if (char === "\u007f" || char === "\b") {
                    if (value.length > 0) {
                        value = value.slice(0, -1)
                        stdout.write("\b \b")
                    }
                    continue
                }

                if (char === "\u001b") continue

                value += char
                stdout.write("•")
            }
        }

        stdout.write(question + " ")
        stdin.setEncoding("utf8")
        stdin.setRawMode(true)
        stdin.resume()
        stdin.on("data", onData)
    })
}

async function validateDiscordToken(token: string): Promise<boolean> {
    try {
        const response = await fetch("https://discord.com/api/v9/users/@me", {
            headers: { Authorization: token },
        })

        return response.ok
    } catch {
        return false
    }
}

function banner(): string {
    const width = 76
    const title = bold(cyan("LyricsStatus")) + " " + dim("terminal setup")
    const subtitle = "Configure the application from the terminal."
    const status = `${green("●")} ${dim("Token, display style, timing, updates")}`

    return [
        frame([
            center(title, width - 4),
            center(dim(subtitle), width - 4),
            "",
            center(status, width - 4),
        ], "Welcome", width),
        ""
    ].join("\n")
}

function startupMenu(): string {
    return frame([
        `${bold(cyan("1"))}  Web panel setup`,
        `${bold(cyan("2"))}  Terminal setup`,
        "",
        dim("Choose the setup style for this launch."),
    ], `${bold("Choose setup mode")}`, 76)
}

function summary(): string {
    const advanced = Settings.view.advanced.enabled ? green("enabled") : dim("disabled")
    const web = dim("legacy web panel is optional via --web")

    return frame([
        `${cyan("Token:")} ${dim(Settings.credentials.token ? "saved" : "missing")}`,
        `${cyan("Timestamp:")} ${Settings.view.timestamp ? green("on") : dim("off")}`,
        `${cyan("Label:")} ${Settings.view.label ? green("on") : dim("off")}`,
        // Emoji is optional and intentionally empty by default.
        `${cyan("Emoji: ")} ${Settings.view.emoji ? green(Settings.view.emoji) : dim("none")}`,
        `${cyan("Advanced:")} ${advanced}`,
        `${cyan("Offset:")} ${yellow(`${Settings.timings.sendTimeOffset} ms`)}`,
        `${cyan("Autooffset:")} ${Settings.timings.enableAutooffset ? green(`${Settings.timings.autooffset} samples`) : dim("off")}`,
        `${cyan("Auto update:")} ${Settings.update.enableAutoupdate ? green("on") : dim("off")}`,
        `${cyan("Web panel:")} ${web}`,
    ], `${bold("Setup complete")}`, 76)
}

export class Tui {
    private static _dashboardListenerAttached = false
    private static _dashboardSuspended = false
    /** Callback invoked when user presses Ctrl+W in the dashboard. */
    public static onOpenWeb: (() => void) | null = null

    public static isDashboardSuspended(): boolean {
        return Tui._dashboardSuspended
    }

    private static setDashboardSuspended(v: boolean): void {
        Tui._dashboardSuspended = v
    }

    private static attachDashboardListener(): void {
        if (Tui._dashboardListenerAttached) return
        if (!stdin.isTTY || !stdout.isTTY) return

        const onData = (chunk: Buffer | string): void => {
            if (Tui._dashboardSuspended) return

            const buf = typeof chunk === "string" ? Buffer.from(chunk, "utf8") : chunk
            for (const b of buf) {
                if (b === 0x03) {
                    try {
                        Tui.detachDashboardListener()
                    } catch {}
                    try { if (stdin.isTTY) stdin.setRawMode(false) } catch {}
                    process.exit(0)
                }

                if (b === 0x13) {
                    ;(async () => {
                        try {
                            Tui.setDashboardSuspended(true)
                            Tui.detachDashboardListener()
                            await Tui.runUpdateSettingsWizard()
                        } catch {
                        } finally {
                            Tui.attachDashboardListener()
                            Tui.setDashboardSuspended(false)
                        }
                    })()
                    continue
                }

                if (b === 0x02) {
                    ;(async () => {
                        try {
                            Tui.setDashboardSuspended(true)
                            Tui.detachDashboardListener()
                            if (Tui.onOpenWeb) Tui.onOpenWeb()
                        } catch {
                        } finally {
                            Tui.attachDashboardListener()
                            Tui.setDashboardSuspended(false)
                        }
                    })()
                    continue
                }
            }
        }

        Tui._onData = onData

        stdin.setEncoding("utf8")
        if (stdin.isTTY) {
            stdin.setRawMode(true)
            stdin.resume()
        }

        stdin.on("data", Tui._onData)
        Tui._dashboardListenerAttached = true
    }

    private static detachDashboardListener(): void {
        if (!Tui._dashboardListenerAttached) return
        if (Tui._onData) stdin.off("data", Tui._onData)
        Tui._onData = undefined

        if (stdin.isTTY) {
            try {
                stdin.setRawMode(false)
            } catch {
                // ignore if raw mode cannot be toggled
            }
        }

        Tui._dashboardListenerAttached = false
    }

    private static _onData: ((chunk: Buffer | string) => void) | undefined

    public static async chooseStartupMode(): Promise<StartupMode> {
        if (!stdin.isTTY || !stdout.isTTY) return "web"

        clearScreen()
        console.log(banner())
        console.log(startupMenu())

        while (true) {
            const answer = (await promptLine("Choose 1 for web or 2 for terminal", "2")).toLowerCase()

            if (["1", "web", "w"].includes(answer)) return "web"
            if (["2", "terminal", "t"].includes(answer)) return "terminal"

            console.log(yellow("Please choose web or terminal."))
        }
    }

    public static async runTerminalFlow(): Promise<void> {
        if (!stdin.isTTY || !stdout.isTTY) {
            throw new Error("Terminal mode requires an interactive terminal.")
        }

        if (!Settings.credentials.token) {
            await Tui.runSetupWizard()
            return
        }

        clearScreen()
        console.log(banner())
        console.log(frame([
            `${bold(cyan("1"))}  Full setup`,
            `${bold(cyan("2"))}  Update settings`,
            `${bold(cyan("3"))}  Start now`,
            "",
            dim("Pick what you want to edit before the dashboard starts."),
        ], `${bold("Terminal mode")}`, 76))

        while (true) {
            const answer = (await promptLine("Choose 1, 2, or 3", "3")).toLowerCase()

            if (["1", "full", "setup"].includes(answer)) {
                await Tui.runSetupWizard()
                return
            }

            if (["2", "update", "updates"].includes(answer)) {
                await Tui.runUpdateSettingsWizard()
                return
            }

            if (["3", "start", "now"].includes(answer)) return

            console.log(yellow("Please choose 1, 2, or 3."))
        }
    }

    public static renderRuntimeDashboard(state: RuntimeSnapshot): void {
        Tui.attachDashboardListener()

        const song = state.songName || "—"
        const artist = state.songAuthor || "—"
        const elapsed = Tui.formatSeconds(Math.floor(state.songProgress / 1000))
        const lyrics = state.currentLine?.text || "Waiting for lyrics..."
        const source = state.lastFetchedFrom || "—"

        // Keep the dashboard compact while still showing the active emoji when configured.
        const emoji = Settings.view.emoji ? `${Settings.view.emoji} ` : ""

        const lines: string[] = []

        lines.push(`${bold(cyan("Song"))}   ${dim("│")} ${emoji}${song}`)
        lines.push(`${bold(cyan("Artist"))} ${dim("│")} ${artist}`)

        if (Settings.view.timestamp) {
            lines.push(`${bold(cyan("Time"))}   ${dim("│")} ${yellow(elapsed)}`)
        }

        lines.push(`${bold(cyan("Ping"))}   ${dim("│")} ${yellow(state.latency !== undefined ? `${state.latency} ms` : "—")}`)

        // The lyrics row stays visible so the current text remains readable in the preview.
        lines.push(`${bold(cyan("Lyrics"))} ${dim("│")} ${green(lyrics)}`)

        lines.push(`${bold(cyan("Source"))} ${dim("│")} ${magenta(source)}`)
        lines.push("")
        lines.push(`${dim("Tip: ")}Press ${bold("Ctrl+S")} to open settings, ${bold("Ctrl+B")} to open the web panel.`)

        clearScreen()
        console.log(frame(lines, `${bold(cyan("LyricsStatus"))}`, 84))
    }

    public static async runSetupWizard(): Promise<void> {
        if (!stdin.isTTY || !stdout.isTTY) {
            throw new Error("The setup wizard requires an interactive terminal.")
        }

        clearScreen()
        console.log(banner())

        console.log(frame([
            `${dim("Step 1")} ${bold("Discord token")}`,
            "This token is stored locally in settings.json and is only used to update your custom status.",
        ], "Account", 76))

        const existingToken = Settings.credentials.token
        let token = existingToken

        if (!token || !(await promptConfirm("Reuse the saved Discord token?", true))) {
            token = await promptSecret(yellow("Enter your Discord token:"))
        }

        while (!(await validateDiscordToken(token))) {
            console.log(yellow("That token did not validate. Try again."))
            token = await promptSecret(yellow("Enter your Discord token:"))
        }

        Settings.credentials.token = token

        console.log("")
        console.log(frame([
            `${dim("Step 2")} ${bold("Status style")}`,
            "Choose how the song text appears in Discord.",
        ], "Preview", 76))

        Settings.view.timestamp = await promptConfirm("Show playback timestamp?", Settings.view.timestamp)
        Settings.view.label = await promptConfirm("Show the Song lyrics label?", Settings.view.label)
        // Optional emoji to show next to the song/status. Empty = none.
        Settings.view.emoji = await promptOptionalLine("Show an emoji?", Settings.view.emoji)

        const enableAdvanced = await promptConfirm("Enable advanced custom status template?", Settings.view.advanced.enabled)
        Settings.view.advanced.enabled = enableAdvanced

        if (enableAdvanced) {
            Settings.view.advanced.customEmoji = await promptOptionalLine("Custom emoji", Settings.view.advanced.customEmoji)
            Settings.view.advanced.customStatus = await promptLine(
                "Custom status template",
                Settings.view.advanced.customStatus
            )
        }

        console.log("")
        console.log(frame([
            `${dim("Step 3")} ${bold("Timing")}`,
            "Fine-tune how early the status changes relative to the lyric timestamp.",
        ], "Sync", 76))

        Settings.timings.sendTimeOffset = await promptNumber("Send time offset (ms)", Settings.timings.sendTimeOffset, 0, 10000)
        Settings.timings.enableAutooffset = await promptConfirm("Enable autooffset?", Settings.timings.enableAutooffset)
        Settings.timings.autooffset = await promptNumber("Autooffset samples", Settings.timings.autooffset, 1, 20)

        console.log("")
        console.log(frame([
            `${dim("Step 4")} ${bold("Updates")}`,
            "Optional update checks keep the beta build aligned with the latest fixes.",
        ], "Maintenance", 76))

        Settings.update.enableAutoupdate = await promptConfirm("Enable automatic update checks?", Settings.update.enableAutoupdate)

        Settings.save()

        console.log("")
        console.log(summary())
        console.log("")
        console.log(dim("Press Enter to continue, or Ctrl+C to exit."))
        await promptLine("")
    }

    public static async runUpdateSettingsWizard(): Promise<void> {
        if (!stdin.isTTY || !stdout.isTTY) {
            throw new Error("The settings editor requires an interactive terminal.")
        }

        while (true) {
            clearScreen()
            console.log(banner())

            console.log(frame([
                `${bold(cyan("1"))}  Account (Discord token)`,
                `${bold(cyan("2"))}  View (timestamp, label, advanced)`,
                `${bold(cyan("3"))}  Timing (offset, autooffset)`,
                `${bold(cyan("4"))}  Updates (auto-update)`,
                "",
                dim("Edit the settings you configured during startup."),
            ], `${bold("Settings editor")}`, 76))

            console.log(frame([
                `${cyan("Current summary:")}`,
                `${cyan("Token: ")} ${dim(Settings.credentials.token ? "saved" : "missing")}`,
                `${cyan("Timestamp: ")} ${Settings.view.timestamp ? green("on") : dim("off")}`,
                `${cyan("Label: ")} ${Settings.view.label ? green("on") : dim("off")}`,
                `${cyan("Emoji: ")} ${Settings.view.emoji ? green(Settings.view.emoji) : dim("none")}`,
                `${cyan("Auto update: ")} ${Settings.update.enableAutoupdate ? green("on") : dim("off")}`,
            ], `${bold("Summary")}`, 76))

            console.log(dim("Choose a section to edit, or press Enter to finish."))
            const choice = (await promptLine("Choose 1-4 or Enter to save/exit", "")).trim()

            if (!choice) {
                Settings.save()
                clearScreen()
                console.log(frame([
                    "Settings saved.",
                ], `${bold("Saved")}`, 76))
                console.log("")
                console.log(dim("Press Enter to continue, or Ctrl+C to exit."))
                await promptLine("")
                return
            }

            if (["1", "account"].includes(choice.toLowerCase())) {
                // Account: change/validate token
                clearScreen()
                console.log(banner())
                console.log(frame([
                    `${dim("Account")}`,
                    "Change the Discord token used to update your custom status.",
                ], "Account", 76))

                const reuse = await promptConfirm("Reuse the saved Discord token?", !!Settings.credentials.token)
                let token = Settings.credentials.token

                if (!reuse) {
                    token = await promptSecret(yellow("Enter your Discord token:"))

                    while (!(await validateDiscordToken(token))) {
                        console.log(yellow("That token did not validate. Try again or press Ctrl+C to cancel."))
                        token = await promptSecret(yellow("Enter your Discord token:"))
                    }
                }

                Settings.credentials.token = token
                Settings.save()
                console.log("")
                console.log(frame([`${cyan("Token saved.")}`], `${bold("Saved")}`, 76))
                console.log("")
                console.log(dim("Press Enter to continue."))
                await promptLine("")
                continue
            }

            if (["2", "view"].includes(choice.toLowerCase())) {
                clearScreen()
                console.log(banner())
                console.log(frame([
                    `${dim("View settings")}`,
                    "Toggle how status text is composed.",
                ], "View", 76))

                Settings.view.timestamp = await promptConfirm("Show playback timestamp?", Settings.view.timestamp)
                Settings.view.label = await promptConfirm("Show the Song lyrics label?", Settings.view.label)
                Settings.view.emoji = await promptOptionalLine("Show an emoji?", Settings.view.emoji)

                const enableAdvanced = await promptConfirm("Enable advanced custom status template?", Settings.view.advanced.enabled)
                Settings.view.advanced.enabled = enableAdvanced

                if (enableAdvanced) {
                    Settings.view.advanced.customEmoji = await promptOptionalLine("Custom emoji", Settings.view.advanced.customEmoji)
                    Settings.view.advanced.customStatus = await promptLine("Custom status template", Settings.view.advanced.customStatus)
                }

                Settings.save()
                console.log("")
                console.log(frame([`${cyan("View settings saved.")}`], `${bold("Saved")}`, 76))
                console.log("")
                console.log(dim("Press Enter to continue."))
                await promptLine("")
                continue
            }

            if (["3", "timing"].includes(choice.toLowerCase())) {
                clearScreen()
                console.log(banner())
                console.log(frame([
                    `${dim("Timing")}`,
                    "Configure status send offsets and autooffset behavior.",
                ], "Timing", 76))

                Settings.timings.sendTimeOffset = await promptNumber("Send time offset (ms)", Settings.timings.sendTimeOffset, 0, 10000)
                Settings.timings.enableAutooffset = await promptConfirm("Enable autooffset?", Settings.timings.enableAutooffset)
                Settings.timings.autooffset = await promptNumber("Autooffset samples", Settings.timings.autooffset, 1, 20)

                Settings.save()
                console.log("")
                console.log(frame([`${cyan("Timing saved.")}`], `${bold("Saved")}`, 76))
                console.log("")
                console.log(dim("Press Enter to continue."))
                await promptLine("")
                continue
            }

            if (["4", "update"].includes(choice.toLowerCase())) {
                clearScreen()
                console.log(banner())
                console.log(frame([
                    `${dim("Updates")}`,
                    "Automatic update checks and maintenance options.",
                ], "Updates", 76))

                Settings.update.enableAutoupdate = await promptConfirm("Enable automatic update checks?", Settings.update.enableAutoupdate)
                Settings.save()

                console.log("")
                console.log(frame([`${cyan("Update settings saved.")}`], `${bold("Saved")}`, 76))
                console.log("")
                console.log(dim("Press Enter to continue."))
                await promptLine("")
                continue
            }

            console.log(yellow("Unknown choice — pick 1-4 or press Enter to finish."))
            await promptLine("")
        }
    }

    public static formatSeconds(totalSeconds: number): string {
        const minutes = Math.floor(totalSeconds / 60)
        const seconds = totalSeconds % 60

        return `${minutes}:${seconds.toString().padStart(2, "0")}`
    }
}
