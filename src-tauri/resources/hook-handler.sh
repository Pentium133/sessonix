#!/bin/bash
# Sessonix hook handler — receives agent hook events (Claude Code, Codex,
# Gemini CLI, Cursor) as JSON on stdin and writes per-session status files to
# ~/.sessonix/hooks/<pty_id>.json for the PTY Sessonix spawned
# (SESSONIX_PTY_ID).
#
# $1 = agent the hook was installed for (claude, codex, gemini, cursor).
# Events from an agent other than the one Sessonix spawned in this PTY
# (SESSONIX_AGENT; e.g. `codex exec` run as a tool inside a Claude session)
# don't touch the status file.
#
# Gemini and Cursor parse the hook's stdout as JSON (Cursor's
# beforeSubmitPrompt needs {"continue": true}), and these hooks also fire
# outside Sessonix. The response comes from bash builtins alone, so it is
# printed even without python3 and costs nothing outside Sessonix.
HOOK_AGENT="$1"
IFS= read -r -d '' INPUT || true
case "$HOOK_AGENT" in
    gemini) printf '{}\n' ;;
    cursor)
        re='"hook_event_name"[[:space:]]*:[[:space:]]*"beforeSubmitPrompt"'
        if [[ $INPUT =~ $re ]]; then printf '{"continue": true}\n'; else printf '{}\n'; fi ;;
esac
[ -z "$SESSONIX_PTY_ID" ] && exit 0
if [ -n "$HOOK_AGENT" ] && [ -n "$SESSONIX_AGENT" ] && [ "$HOOK_AGENT" != "$SESSONIX_AGENT" ]; then
    exit 0
fi
command -v python3 >/dev/null 2>&1 || exit 0
printf '%s' "$INPUT" | SESSONIX_HOOK_AGENT="$HOOK_AGENT" python3 -c "
import sys, json, time, os

agent = os.environ.get('SESSONIX_HOOK_AGENT', '')
try:
    data = json.load(sys.stdin)
    pty_id = os.environ.get('SESSONIX_PTY_ID', '')
    if not isinstance(data, dict) or not pty_id.isdigit():
        sys.exit(0)
    event = data.get('hook_event_name', '')
    if not event:
        sys.exit(0)
    session_id = data.get('session_id', '') or data.get('conversation_id', '')
    error = None

    if agent == 'gemini':
        # https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/reference.md
        status = {
            'SessionStart': 'idle',
            'BeforeAgent': 'running',
            'BeforeTool': 'running',
            'AfterTool': 'running',
            'PreCompress': 'running',
            'AfterAgent': 'idle',
            'SessionEnd': 'exited',
        }.get(event, '')
        if event == 'Notification' and data.get('notification_type') == 'ToolPermission':
            status = 'waiting_permission'
    elif agent == 'cursor':
        # https://cursor.com/docs/hooks — permission hooks are never installed.
        status = {
            'sessionStart': 'idle',
            'beforeSubmitPrompt': 'running',
            'postToolUse': 'running',
            'postToolUseFailure': 'running',
            'afterShellExecution': 'running',
            'afterMCPExecution': 'running',
            'afterFileEdit': 'running',
            'preCompact': 'running',
            'sessionEnd': 'exited',
        }.get(event, '')
        if event == 'stop':
            if data.get('status') == 'error':
                status, error = 'error', 'agent error'
            else:
                status = 'idle'
    else:
        # Claude Code (https://code.claude.com/docs/en/hooks) and Codex
        # (https://learn.chatgpt.com/docs/hooks) share event names.
        status = {
            'SessionStart': 'idle',
            'UserPromptSubmit': 'running',
            # Tool and subagent events are heartbeats during long turns; they
            # also re-mark running after a permission answer or an overridden Stop.
            'PreToolUse': 'running',
            'PostToolUse': 'running',
            'PostToolUseFailure': 'running',
            'PermissionDenied': 'running',
            'SubagentStart': 'running',
            'PreCompact': 'running',
            'PostCompact': 'running',
            'Stop': 'idle',
            # Codex: the user interrupted the active turn (Claude fires nothing).
            'Interrupt': 'idle',
            # Claude: fires instead of Stop when the turn ends on an API error.
            'StopFailure': 'error',
            'PermissionRequest': 'waiting_permission',
            'SessionEnd': 'exited',
        }.get(event, '')
        # Auto-compaction mid-turn re-fires SessionStart; the turn goes on.
        if event == 'SessionStart' and data.get('source') == 'compact':
            status = 'running'
        if event == 'Notification':
            status = {
                'permission_prompt': 'waiting_permission',
                'elicitation_dialog': 'waiting_permission',
                'elicitation_url_dialog': 'waiting_permission',
                'agent_needs_input': 'waiting_permission',
                'idle_prompt': 'idle',
            }.get(data.get('notification_type', ''), '')
        if event == 'StopFailure':
            error = data.get('error', '')
    if not status:
        sys.exit(0)

    os.makedirs(os.path.expanduser('~/.sessonix/hooks'), exist_ok=True)
    path = os.path.expanduser(f'~/.sessonix/hooks/{pty_id}.json')
    now_ms = int(time.time() * 1000)
    try:
        with open(path) as f:
            prev = json.load(f)
    except (OSError, ValueError):
        prev = {}
    # Codex also fires SessionStart for subagents mid-turn: never let it end a
    # running turn. (Claude's SessionStart does reset a stale running state.)
    if (agent == 'codex' and event == 'SessionStart' and status == 'idle'
            and prev.get('status') == 'running'):
        sys.exit(0)
    # Hooks may run async, so a tool event's handler can finish after the turn
    # end or permission wait that followed it. Don't let such a late heartbeat
    # overwrite a fresh one.
    heartbeats = ('PreToolUse', 'PostToolUse', 'PostToolUseFailure', 'PermissionDenied',
                  'SubagentStart', 'BeforeTool', 'AfterTool', 'postToolUse',
                  'postToolUseFailure', 'afterShellExecution', 'afterMCPExecution',
                  'afterFileEdit')
    settled = ('Stop', 'Interrupt', 'PermissionRequest', 'AfterAgent', 'stop')
    if (event in heartbeats and prev.get('event') in settled
            and now_ms - int(prev.get('ts_ms', 0)) < 2000):
        sys.exit(0)
    out = {
        'event': event,
        'status': status,
        'session_id': session_id,
        'pty_id': int(pty_id),
        'ts': now_ms // 1000,
        'ts_ms': now_ms,
    }
    if error is not None:
        out['error'] = error
    tmp = f'{path}.{os.getpid()}.tmp'
    with open(tmp, 'w') as f:
        json.dump(out, f)
    os.rename(tmp, path)
except Exception:
    pass
" 2>/dev/null
exit 0
