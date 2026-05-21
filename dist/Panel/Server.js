"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.startServer = startServer;
const express_1 = __importDefault(require("express"));
const node_http_1 = require("node:http");
const ws_1 = require("ws");
const node_path_1 = require("node:path");
const Settings_1 = require("../Settings");
const Debug_1 = require("../Debug");
let _serverStarted = false;
let _httpServer;
let _wss;
let _activeClients = 0;
let _shutdownTimer = null;
function startServer() {
    if (_serverStarted) {
        Debug_1.Debug.write("Web panel already started; ignoring duplicate start request.");
        return;
    }
    const app = (0, express_1.default)();
    const httpServer = (0, node_http_1.createServer)(app);
    _httpServer = httpServer;
    app.use("/", express_1.default.static((0, node_path_1.join)(__dirname, "../../static")));
    app.get("/", (req, res) => {
        res.sendFile((0, node_path_1.join)(__dirname, "../../static/index.html"));
    });
    try {
        const wss = new ws_1.WebSocketServer({ server: httpServer, path: "/ws" });
        _wss = wss;
        wss.on("connection", (ws) => {
            _activeClients++;
            Debug_1.Debug.write(`Web panel client connected (${_activeClients} active)`);
            if (_shutdownTimer) {
                clearTimeout(_shutdownTimer);
                _shutdownTimer = null;
                Debug_1.Debug.write("Cancelled web panel shutdown (new client connected)");
            }
            ws.on("message", (data) => {
                try {
                    const settings = JSON.parse(data.toString());
                    Settings_1.Settings.credentials = settings.credentials;
                    Settings_1.Settings.view = settings.view;
                    Settings_1.Settings.timings = settings.timings;
                    Settings_1.Settings.update = settings.update;
                    Settings_1.Settings.save();
                }
                catch (e) {
                    Debug_1.Debug.write(`Failed to parse settings from websocket: ${e.stack}`);
                }
            });
            ws.on("close", () => {
                _activeClients = Math.max(0, _activeClients - 1);
                Debug_1.Debug.write(`Web panel client disconnected (${_activeClients} remaining)`);
                if (_activeClients === 0) {
                    _shutdownTimer = setTimeout(() => {
                        Debug_1.Debug.write("No web panel clients — shutting down web panel to save resources.");
                        try {
                            if (_wss) {
                                try {
                                    _wss.close();
                                }
                                catch (_a) { }
                                _wss = undefined;
                            }
                            if (_httpServer) {
                                try {
                                    _httpServer.close(() => Debug_1.Debug.write("Web panel HTTP server closed."));
                                }
                                catch (e) {
                                    Debug_1.Debug.write(`Error closing HTTP server: ${e.stack}`);
                                }
                                _httpServer = undefined;
                            }
                            _serverStarted = false;
                        }
                        catch (e) {
                            Debug_1.Debug.write(`Error while shutting down web panel: ${e.stack}`);
                        }
                    }, 15000);
                }
            });
            ws.send(JSON.stringify({
                credentials: Settings_1.Settings.credentials,
                view: Settings_1.Settings.view,
                timings: Settings_1.Settings.timings,
                update: Settings_1.Settings.update
            }));
        });
    }
    catch (e) {
        Debug_1.Debug.write(`Failed to start WebSocket server: ${e.stack}`);
    }
    httpServer.on("listening", () => {
        _serverStarted = true;
        Debug_1.Debug.write("Web panel listening on http://localhost:8999");
        console.log("Web panel listening on http://localhost:8999");
    });
    httpServer.on("error", (err) => {
        var _a;
        Debug_1.Debug.write(`Web panel HTTP server error: ${(_a = err.stack) !== null && _a !== void 0 ? _a : err}`);
        console.error("Web panel HTTP server error:", err);
    });
    try {
        httpServer.listen(8999);
    }
    catch (e) {
        Debug_1.Debug.write(`Failed to listen on port 8999: ${e.stack}`);
    }
}
