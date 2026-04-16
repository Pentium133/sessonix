#!/bin/bash
# Sessonix hook handler — receives Claude Code hook events via stdin.
# Writes per-session status files to ~/.sessonix/hooks/<pty_id>.json
# Set SESSONIX_PTY_ID env var when spawning Claude to enable.
PTY_ID="${SESSONIX_PTY_ID}"
[ -z "$PTY_ID" ] && exit 0
mkdir -p ~/.sessonix/hooks 2>/dev/null
python3 -c "
import sys, json, time, os
try:
    data = json.load(sys.stdin)
    event = data.get('hook_event_name', '')
    session_id = data.get('session_id', '')
    pty_id = os.environ.get('SESSONIX_PTY_ID', '')
    if not event or not pty_id or not pty_id.isdigit():
        sys.exit(0)
    status_map = {
        'SessionStart': 'idle',
        'UserPromptSubmit': 'running',
        'Stop': 'idle',
        'PermissionRequest': 'waiting_permission',
        'SessionEnd': 'exited',
        'PreCompact': 'running',
    }
    status = status_map.get(event, '')
    if event == 'Notification':
        matcher = data.get('matcher', '')
        if matcher in ('permission_prompt', 'elicitation_dialog'):
            status = 'waiting_permission'
    if not status:
        sys.exit(0)
    out = {
        'event': event,
        'status': status,
        'session_id': session_id,
        'pty_id': int(pty_id),
        'ts': int(time.time()),
    }
    path = os.path.expanduser(f'~/.sessonix/hooks/{pty_id}.json')
    tmp = path + '.tmp'
    with open(tmp, 'w') as f:
        json.dump(out, f)
    os.rename(tmp, path)
except:
    pass
" 2>/dev/null
exit 0
