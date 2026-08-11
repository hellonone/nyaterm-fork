---
sidebar_position: 0
---

# Session Types

NyaTerm is not just an SSH client. It is a desktop app that puts multiple terminal and remote-desktop workflows into one workspace. It currently supports five session types:

- **SSH**
- **Local Terminal**
- **Telnet**
- **Serial**
- **RDP**

Understanding the differences helps explain why some panels or enhancements only appear for certain tabs.

## At a glance

| Session Type | Typical scenario | Key capabilities |
|--------------|------------------|------------------|
| SSH | Remote Linux / Unix administration | SFTP, OTP, resource / GPU / process / Docker monitoring, proxy, jump host, tunnels, algorithm preferences |
| Local Terminal | Local shell work, scripts, builds | Shared terminal UI, command history, split panes |
| Telnet | Legacy devices, lab environments, compatibility troubleshooting | Terminal workspace features with `Backspace Mode`, but not SSH-only features |
| Serial | Routers, switches, boards, embedded debug ports | Serial port settings, `Backspace Mode`, and terminal workspace features |
| RDP | Windows Remote Desktop or graphical administration entry points | Remote desktop display, NLA/CredSSP, certificate verification, text clipboard, window fitting, and reconnects |

## SSH

SSH is the most capable session type in NyaTerm. It is the best fit when you need to:

- Log in to remote Linux / Unix hosts
- Browse and transfer remote files
- Use OTP, jump hosts, or proxies
- Watch remote resource, GPU, process, and Docker monitoring
- Configure port tunnels
- Fine-tune negotiated SSH algorithms

If you need any of these, use **SSH** first:

- File explorer
- Auto-upload / round-trip editing
- Remote resource monitoring
- SSH tunnels in the Network panel

## Local Terminal

Local Terminal is useful when you want your local shell workflow inside the same NyaTerm workspace, for example:

- Running frontend or Rust builds locally
- Running scripts, reading logs, or using Git
- Comparing local and remote output side by side

Its value is not remote access. Its value is that it shares the same workspace model as SSH sessions:

- Tabs
- Split panes
- Terminal search
- Command history and suggestions
- Optional line numbers, timestamps, and highlighting

When creating a local terminal, you can also choose:

- The shell path, such as `powershell.exe`, `cmd.exe`, `bash`, or `wsl.exe`
- The working directory

## Telnet

Telnet is useful for:

- Maintaining older equipment
- Lab environments
- Compatibility scenarios where SSH is not available

You still get NyaTerm's terminal workspace model, but not SSH-specific security or file features. In practice, that usually means no:

- SFTP file explorer
- OTP binding
- SSH jump host
- SSH resource monitoring

If your goal is simply to open a traditional remote terminal quickly, Telnet can be the more direct choice.

For devices that expect specific erase behavior, Telnet also exposes `Backspace Mode` so you can choose `Ctrl+H (BS)` or `DEL (0x7F)`.

## Serial

Serial sessions are useful for connecting to:

- Network device console ports
- Routers and switches
- Development boards, embedded devices, and debug ports

When creating a serial session, you can configure:

- Port
- Baud rate
- Data bits
- Parity
- Stop bits
- `Backspace Mode`

Serial sessions still live inside NyaTerm's tabbed and split workspace, so you can watch serial output in one pane while running commands in an SSH or local terminal pane.

## RDP

RDP sessions are for Windows hosts or other environments that expose a Remote Desktop endpoint. They share NyaTerm's saved-connection, tab, and split-pane workspace model, but the underlying session is a graphical desktop instead of a text terminal.

When creating an RDP session, you can configure:

- Host, port, username, password, and domain
- Network Level Authentication (NLA / CredSSP)
- Certificate policy: ask on unknown certificates, strict rejection, or accept for this session
- Display mode: fit to window or fixed size
- Text clipboard mode
- Automatic reconnect attempts

When connecting to an RDP host with an unknown certificate, NyaTerm opens a certificate verification dialog. You can accept the certificate for the current connection only or accept and remember it. If a remembered certificate changes later, NyaTerm prompts again before connecting.

RDP does not provide terminal command history, the SFTP file explorer, SSH proxy/jump-host behavior, or remote resource monitoring. If you need command-line enhancements, use SSH, Local Terminal, Telnet, or Serial instead.

## How to choose

A simple rule of thumb:

- Need the full remote workflow? Use **SSH**
- Need a local shell? Use **Local Terminal**
- Need a traditional remote terminal? Use **Telnet**
- Need a device console or debug port? Use **Serial**
- Need a graphical Windows remote desktop? Use **RDP**

## Mix them in one workspace

One of NyaTerm's strengths is that you can mix these session types in the same workspace, for example:

- SSH on the left to watch remote logs
- Local Terminal on the right to run packaging or Git commands
- A Serial tab open to watch device boot output
- An RDP pane open to inspect a Windows remote desktop

That is why some features are documented as session-specific. The workspace is shared, but the capability boundary still depends on the underlying session type.

:::tip Screenshot suggestion
- Suggested image path: `/img/docs/session-types/new-session-tabs.png`
- Show the SSH / Local Terminal / Telnet / Serial tabs in the new-session window
- Keeping the default field areas visible helps readers understand the differences
:::
