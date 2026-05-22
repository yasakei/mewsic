#!/usr/bin/env node
const fs = require('fs')
const path = require('path')
const os = require('os')

const pidFile = path.join(os.homedir(), '.config', 'lyrics-status', 'lyrics-status.pid')

if (!fs.existsSync(pidFile)) {
  console.log('No running lyrics-status found.')
  process.exit(0)
}

const raw = fs.readFileSync(pidFile, 'utf8')
const pid = Number(raw.trim())

if (Number.isNaN(pid)) {
  try { fs.unlinkSync(pidFile) } catch {}
  console.log('Removed invalid pidfile.')
  process.exit(0)
}

try {
  process.kill(pid, 'SIGTERM')
  console.log(`Sent SIGTERM to process ${pid}`)
} catch (e) {
  console.error(`Failed to stop pid ${pid}: ${(e && e.message) || e}`)
}

try { fs.unlinkSync(pidFile) } catch {}

process.exit(0)
