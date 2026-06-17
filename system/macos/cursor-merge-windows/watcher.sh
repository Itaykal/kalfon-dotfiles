#!/usr/bin/env bash
# Poll for Cursor.app. On rising edge (not-running -> running), trigger merge.applescript.
# Designed to be run as a launchd LaunchAgent with RunAtLoad + KeepAlive.

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APPLESCRIPT="$SCRIPT_DIR/merge.applescript"

prev=0
while true; do
	if /usr/bin/pgrep -x Cursor >/dev/null 2>&1; then
		curr=1
	else
		curr=0
	fi
	if [ "$curr" = 1 ] && [ "$prev" = 0 ]; then
		/usr/bin/osascript "$APPLESCRIPT" >/dev/null 2>&1 || true
	fi
	prev=$curr
	sleep 2
done
