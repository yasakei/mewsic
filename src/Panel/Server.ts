import express from "express"
import { createServer } from "node:http"
import { WebSocketServer, WebSocket } from "ws"
import { join } from "node:path"
import { Settings } from "../Settings"
import { Debug } from "../Debug"

let _serverStarted = false
let _httpServer: ReturnType<typeof createServer> | undefined
let _wss: WebSocketServer | undefined
let _activeClients = 0
let _shutdownTimer: NodeJS.Timeout | null = null

export function startServer(): void {
    if (_serverStarted) {
        Debug.write("Web panel already started; ignoring duplicate start request.")
        return
    }

    const app = express()
    const httpServer = createServer(app)
    _httpServer = httpServer

    app.use("/", express.static(join(__dirname, "../../static")))

    app.get("/", (req, res) => {
        res.sendFile(join(__dirname, "../../static/index.html"))
    })

    try {
        const wss = new WebSocketServer({ server: httpServer, path: "/ws" })
        _wss = wss

        wss.on("connection", (ws: WebSocket) => {
            _activeClients++
            Debug.write(`Web panel client connected (${_activeClients} active)`) 

            if (_shutdownTimer) {
                clearTimeout(_shutdownTimer)
                _shutdownTimer = null
                Debug.write("Cancelled web panel shutdown (new client connected)")
            }

            ws.on("message", (data) => {
                try {
                    const settings = JSON.parse(data.toString())

                    Settings.credentials = settings.credentials
                    Settings.view        = settings.view
                    Settings.timings     = settings.timings
                    Settings.update      = settings.update

                    Settings.save()
                } catch (e) {
                    Debug.write(`Failed to parse settings from websocket: ${(e as Error).stack}`)
                }
            })

            ws.on("close", () => {
                _activeClients = Math.max(0, _activeClients - 1)
                Debug.write(`Web panel client disconnected (${_activeClients} remaining)`)

                if (_activeClients === 0) {
                    _shutdownTimer = setTimeout(() => {
                        Debug.write("No web panel clients — shutting down web panel to save resources.")
                        try {
                            if (_wss) {
                                try { _wss.close() } catch {}
                                _wss = undefined
                            }

                            if (_httpServer) {
                                try { _httpServer.close(() => Debug.write("Web panel HTTP server closed.")) } catch (e) { Debug.write(`Error closing HTTP server: ${(e as Error).stack}`) }
                                _httpServer = undefined
                            }

                            _serverStarted = false
                        } catch (e) {
                            Debug.write(`Error while shutting down web panel: ${(e as Error).stack}`)
                        }
                    }, 15000)
                }
            })

            ws.send(JSON.stringify({
                credentials: Settings.credentials,
                view:        Settings.view,
                timings:     Settings.timings,
                update:      Settings.update
            }))
        })
    } catch (e) {
        Debug.write(`Failed to start WebSocket server: ${(e as Error).stack}`)
    }

    httpServer.on("listening", () => {
        _serverStarted = true
        Debug.write("Web panel listening on http://localhost:8999")
        console.log("Web panel listening on http://localhost:8999")
    })

    httpServer.on("error", (err) => {
        Debug.write(`Web panel HTTP server error: ${(err as Error).stack ?? err}`)
        console.error("Web panel HTTP server error:", err)
    })

    try {
        httpServer.listen(8999)
    } catch (e) {
        Debug.write(`Failed to listen on port 8999: ${(e as Error).stack}`)
    }
}
