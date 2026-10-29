#!/bin/bash
# Sessonix hook handler — receives Claude Code hook events via stdin.
# Writes per-session status files to ~/.sessonix/hooks/<pty_id>.json
# Set SESSONIX_PTY_ID env var when spawning Claude to enable.
# Event semantics: https://code.claude.com/docs/en/hooks
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
        # Tool and subagent events are heartbeats during long turns; they also
        # re-mark running after a permission answer or an overridden Stop.
        'PreToolUse': 'running',
        'PostToolUse': 'running',
        'PostToolUseFailure': 'running',
        'PermissionDenied': 'running',
        'SubagentStart': 'running',
        'PreCompact': 'running',
        'Stop': 'idle',
        # Fires instead of Stop when the turn ends on an API error.
        'StopFailure': 'error',
        'PermissionRequest': 'waiting_permission',
        'SessionEnd': 'exited',
    }
    status = status_map.get(event, '')
    if event == 'Notification':
        status = {
            'permission_prompt': 'waiting_permission',
            'elicitation_dialog': 'waiting_permission',
            'elicitation_url_dialog': 'waiting_permission',
            'agent_needs_input': 'waiting_permission',
            'idle_prompt': 'idle',
        }.get(data.get('notification_type', ''), '')
    if not status:
        sys.exit(0)
    out = {
        'event': event,
        'status': status,
        'session_id': session_id,
        'pty_id': int(pty_id),
        'ts': int(time.time()),
    }
    if event == 'StopFailure':
        out['error'] = data.get('error', '')
    path = os.path.expanduser(f'~/.sessonix/hooks/{pty_id}.json')
    tmp = path + '.tmp'
    with open(tmp, 'w') as f:
        json.dump(out, f)
    os.rename(tmp, path)
except:
    pass
" 2>/dev/null
exit 0
