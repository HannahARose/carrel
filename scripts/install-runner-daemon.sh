#!/bin/sh
# Runs DeskBook's GitHub Actions runner as a LaunchDaemon, as carrel-ci, so it starts with the
# machine and survives reboots, with or without anyone logged in. It starts runsvc.sh, which
# reads the PATH and environment saved in .path and .env. Run with sudo.
set -eu
plist=/Library/LaunchDaemons/com.horizon-labs.carrel-runner.plist
cat > "$plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.horizon-labs.carrel-runner</string>
  <key>UserName</key><string>carrel-ci</string>
  <key>WorkingDirectory</key><string>/Users/carrel-ci/actions-runner</string>
  <key>ProgramArguments</key>
  <array><string>/Users/carrel-ci/actions-runner/runsvc.sh</string></array>
  <key>EnvironmentVariables</key>
  <dict><key>HOME</key><string>/Users/carrel-ci</string></dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>/Users/carrel-ci/actions-runner/daemon.log</string>
  <key>StandardErrorPath</key><string>/Users/carrel-ci/actions-runner/daemon.log</string>
</dict>
</plist>
PLIST
chown root:wheel "$plist"
chmod 644 "$plist"
# Stop the copy started by hand, so only the daemon runs.
pkill -u carrel-ci -f Runner.Listener || true
pkill -u carrel-ci -f "actions-runner/run.sh" || true
pkill -u carrel-ci -f "actions-runner/runsvc.sh" || true
launchctl bootout system/com.horizon-labs.carrel-runner 2>/dev/null || true
launchctl bootstrap system "$plist"
echo "The runner now runs as a LaunchDaemon (com.horizon-labs.carrel-runner)."
