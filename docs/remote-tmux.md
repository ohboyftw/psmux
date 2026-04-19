# Remote tmux Control

psmux can connect to a tmux session running on a remote Linux/macOS host and
render it locally. You interact with the remote panes using your local keyboard
while psmux keeps the screen in sync over the SSH connection.

## Prerequisites

- **`ssh` on PATH.** psmux spawns the system `ssh` binary — Windows 10 1809+
  ships OpenSSH, or install it via `winget install Microsoft.OpenSSH.Client`.
- **tmux installed on the remote host.** Any recent tmux version works.
- **SSH authentication working from your shell.** Key-based auth is the
  easiest path; psmux passes your credentials straight through to the `ssh`
  binary (agent forwarding, ~/.ssh/config, etc. all apply).
- There is no built-in passphrase prompt. If your key requires a passphrase,
  run `ssh-add` first to load it into an agent.

## Connecting

### Attach to an existing session

```powershell
psmux attach-remote user@host
psmux attach-remote user@host:mysession
```

`user@host` follows standard SSH syntax. Append `:sessionname` to specify
which tmux session to attach to. If omitted, psmux tries to attach to a session
named `default` and creates one if it does not exist.

### Create a new session on the remote host

```powershell
psmux new-session-remote user@host
psmux new-session-remote user@host:myproject
```

Creates (or attaches to) a tmux session with the given name on the remote host.

### List sessions on a remote host

```powershell
psmux list-sessions-remote user@host
```

### Extra SSH options

Pass additional SSH flags after `--`:

```powershell
psmux attach-remote user@host:work -- -i C:\Users\you\.ssh\id_ed25519
psmux attach-remote user@host:work -- -p 2222 -o StrictHostKeyChecking=no
```

Extra options are split on whitespace and prepended to the `ssh` invocation
before the hostname argument.

## How it works

psmux spawns `ssh user@host tmux -CC attach -t sessionname` (or `new-session`
on first connect). tmux on the remote side starts in **control mode** (`-CC`),
which emits structured line-protocol notifications for all pane output and
layout changes.

psmux parses this control-mode stream, maintains a local vt100 screen buffer
per pane, and renders the active pane to your terminal on every update. Your
keyboard input is forwarded as tmux `send-keys` commands over the same SSH
stdin.

The connection sequence:
1. psmux tries `tmux -CC attach -t {session}` — succeeds if the session exists.
2. On failure, retries with `tmux -CC new-session -s {session}`.
3. After connection, sends `refresh-client -C {cols}x{rows}` to match your
   local terminal size.

## Keyboard and interaction

| Key | Action |
|-----|--------|
| Typing | Forwarded to the remote pane as `send-keys` |
| Ctrl+b (prefix) | Arms prefix mode — next key is a tmux binding |
| Ctrl+b d | Detach — leaves the remote session running, returns to local shell |
| Arrow keys, F1–F12, Delete, etc. | Forwarded with correct escape sequences |
| Ctrl+modifier, Alt+modifier | Forwarded as `C-` and `M-` key names |
| Terminal resize | Sends `refresh-client -C` to the remote session |

**Mouse** events are not forwarded. If the remote tmux has mouse mode enabled,
clicks will not work.

## Limitations

The following local-psmux features do not work in remote sessions:

| Feature | Status |
|---------|--------|
| Mouse support | Not forwarded |
| Copy mode | Remote pane's copy mode (in tmux on the server) works, but local copy mode is not available |
| Window switching | The active pane is rendered; switching to other windows requires tmux prefix key bindings sent to the remote session |
| psmux status bar | Not shown — you see the remote tmux's own status bar via the screen render |
| `wait-for`, `exec`, `spawn_agent` | These target local panes only |
| CustomPaneBackend | Connects only to local sessions |

Only one pane (the active one) is rendered locally at a time. The remote
session can have any layout — switching the active pane on the remote side
(via `Ctrl+b Arrow` sent through) causes psmux to render the newly active pane.

## Troubleshooting

### "No remote session found" or connection hangs

Check that `ssh user@host` works without psmux first:

```powershell
ssh user@host tmux -CC new-session -s test
```

If that hangs, the issue is SSH connectivity (firewall, wrong port, key not
accepted). Fix SSH before retrying psmux.

### Screen blank or garbled after connecting

psmux sends `refresh-client -C {cols}x{rows}` immediately after connect. If
the terminal size is misdetected, resize your Windows Terminal window and the
remote session will re-sync on the next resize event.

### SSH drops mid-session

When the SSH connection terminates (network drop, idle timeout, remote server
restart), psmux receives EOF on the SSH stdout stream and exits cleanly, printing:

```
[remote session 'sessionname' error: connection reset]
```

The remote tmux session is unaffected — reattach with the same command. If
your SSH server disconnects idle clients, configure `ServerAliveInterval 30`
in `~/.ssh/config`.

### "Failed to spawn" when ssh is not on PATH

psmux calls the bare `ssh` binary. On fresh Windows installs, open Settings →
Apps → Optional features and install "OpenSSH Client", then open a new terminal.
