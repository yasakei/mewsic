#!/usr/bin/env node
const fs = require('fs')
const path = require('path')
const os = require('os')
const { spawn } = require('child_process')

const pidFile = path.join(os.homedir(), '.config', 'lyrics-status', 'lyrics-status.pid')

if (fs.existsSync(pidFile)) {
  const raw = fs.readFileSync(pidFile, 'utf8')
  const pid = Number(raw.trim())

  if (!Number.isNaN(pid)) {
    try {
      process.kill(pid, 0)
      console.log(`lyrics-status already running (pid ${pid}).`)
      console.log('Run `pnpm stop` to stop it.')
      process.exit(0)
    } catch (e) {
      // stale pidfile, remove and continue
      try { fs.unlinkSync(pidFile) } catch {}
    }
  }
}

// No running instance found — spawn the app
const node = process.execPath
const script = path.join(process.cwd(), 'dist', 'index.js')
const child = spawn(node, [script, ...process.argv.slice(2)], { stdio: 'inherit' })

child.on('exit', (code) => process.exit(code))
