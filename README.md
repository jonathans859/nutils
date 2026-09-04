# NUtils

NUtils is a hotkey-controlled window manager for Windows, built with screen-reader
users in mind. It hides, reveals, and cloaks windows on a keystroke so you can keep
many things open without them cluttering your taskbar, Alt-Tab order, or screen.

Originally written in AutoIt (2008–2010) by **Niko Carpenter** and **Tyler Spivey**.
Version 4 is a ground-up rewrite in **Rust** on the native Win32 API — a single
self-contained ~0.5 MB executable with no runtime or redistributable dependency.
The original AutoIt 3 sources are not kept in the working tree; they remain in
this repository's Git history, before the v4 rewrite (commit `9dd82cc` and
earlier).

## Features

- **Hide / unhide windows into 10 slots** with one keystroke (Ctrl+Shift+1…0).
- **Stacks** — extra sets of 10 slots for when you need to hide more than ten
  windows, or want to organize them; switch stacks with Ctrl+Shift+`=` / `-`.
- **Make a window transparent** — invisible on screen but still fully interactable
  through a screen reader (Win+Shift+`\`), and solid again (Win+Shift+`/`).
- **Auto-transparent apps**: designate an application with **Win+Shift+A**; its
  active window is made transparent at once and any new window or dialog it opens
  is made transparent the instant it appears (with no flash). Stop it again with
  **Win+Shift+S**, which also makes its windows solid. See
  [Auto-transparent apps](#auto-transparent-apps).
- **Kill the active window's process** (Win+F4) for unresponsive apps.
- **Change a window's title** (Win+Shift+T) to tell same-named windows apart.
- **Unhide from the tray** — a "Hidden" submenu lists every hidden window, grouped
  per stack; click one to bring it back.
- **Set the active process's priority** (Ctrl+Shift+F3…F8: low → realtime).
- **WinMurderer** — automatically close or kill windows matching a watch-list.
- **Hidden state survives a restart** of NUtils (but not a reboot — handles are
  meaningless after one, so they are discarded).
- **Feedback your way** — beeps, spoken text, or both. Spoken text goes through
  your **screen reader**: release builds speak via Prism, which reaches NVDA over
  its own RPC endpoint (and also JAWS, ZoomText and Narrator), so a single
  `nutils.exe` copied to another machine still talks. Builds made without the
  `speech` feature use NVDA's `nvdaControllerClient64.dll` if it sits next to the
  exe, and fall back to Windows SAPI otherwise.
- **Optional WAV sound packs** in place of the built-in PC-speaker beeps.
- **Accessible settings editor** — rebind every shortcut in a native wxWidgets
  dialog built for screen readers. See [Settings](#settings).

## Installing

Copy `nutils.exe` and its `lang\` folder into a folder of your choice and run it.
NUtils lives in the system tray. To start it automatically, drop a shortcut to
`nutils.exe` in your Startup folder (`shell:startup`).

## Usage

Default hotkeys (all configurable in `config.toml`):

| Action | Hotkey |
|---|---|
| Hide / unhide window in slot 1–10 | `Ctrl+Shift+1` … `Ctrl+Shift+0` |
| Next / previous stack | `Ctrl+Shift+=` / `Ctrl+Shift+-` |
| Hide in first free slot | `Win+Shift+H` |
| Make transparent / solid | `Win+Shift+\` / `Win+Shift+/` |
| Change active window title | `Win+Shift+T` |
| Kill active window's process | `Win+F4` |
| Process priority (low→realtime) | `Ctrl+Shift+F3` … `Ctrl+Shift+F8` |
| Start auto-transparenting the active window's app | `Win+Shift+A` |
| Stop auto-transparenting it (make it solid again) | `Win+Shift+S` |
| Speak the hidden-window status | `Win+Shift+I` |

Slot `0` is the tenth slot, not slot zero. Pressing a slot's hotkey hides the
active window there if the slot is empty, or brings that window back if occupied.
You cannot unhide a window that is on a different stack without switching to it
first. The desktop, taskbar, and Start menu cannot be hidden.

The tray menu opens with a status line — "3 windows hidden in 2 stacks" — followed
by a **Hidden** submenu listing every hidden window (grouped into per-stack
submenus when they span several stacks); click one to unhide it. Choosing the
status line itself speaks the status, exactly as the status hotkey does.
Configuration reloads automatically within about a second of `config.toml`
changing — from the settings editor or a manual edit — so there is no reload item.

### Hidden-window status

**Win+Shift+I** speaks how many windows are hidden in how many stacks. This is
always spoken, even when feedback is set to `beeps`, since a beep pattern cannot
carry the information; if no speech is available at all it falls back to a single
notification tone.

Turn on **Detailed status** (General tab of the settings editor, or
`detailed_status = true` under `[settings]`) to have it additionally name the
current stack and then every hidden window by stack, position and title:

> 3 windows hidden in 2 stacks. Current stack 1. Stack 1: position 1, Untitled -
> Notepad; position 4, Inbox - Outlook. Stack 2: position 2, Calculator.

Windows with no title at all are announced by their executable name.

Making a window transparent throws off screen readers that rely on display
hooking, so there are limits to that feature. NUtils cannot act on windows running
with higher privileges than itself.

## Auto-transparent apps

Mark an application so that **every new window it opens is instantly made
transparent** — useful when an app is one you keep running "in the background" and
never want to see on screen, even when it pops up a dialog.

Press **Win+Shift+A** on any window of the app (or edit `config.toml`):

```toml
[[managed_apps]]
match = "exe"          # "exe" | "title" | "class"
value = "wxdragon.exe" # case-insensitive
```

To stop, press **Win+Shift+S** on any window of that app: NUtils removes it from
the list, stops the in-process helper, and makes its windows solid again.

Both cues play **three** tones (falling when auto-transparent goes on, rising when
it goes off), against the **two** tones of the plain per-window transparent/solid
cues — so you can hear whether you just changed one window or the whole app. A
sound pack can override them with `autotransparent.wav` / `autosolid.wav`.

The two mechanisms stay out of each other's way, so nothing fights over a window:

- On a window of an auto-transparent app the **per-window** hotkeys
  (`Win+Shift+\` / `Win+Shift+/`) are refused with a low double tone — the app's
  transparency belongs to auto-transparent, which would just re-apply it. Stop
  auto-transparenting the app first.
- **Win+Shift+S** only stops auto-transparenting; on an app that isn't on the
  list it is refused rather than making a hand-transparented window solid.
- **Win+Shift+A** works on a window you made transparent by hand. That window
  keeps its manual state: when you later press Win+Shift+S the app's other
  windows go solid and this one stays transparent until you press `Win+Shift+/`.

This works in two layers:

- A standard `SetWinEventHook` accessibility watcher running inside NUtils' own
  process notices when a managed app's window appears and cloaks it. This alone
  can let a window flash for a single frame if the app paints it the instant it is
  shown.
- To make hiding **flash-free**, NUtils then loads a tiny helper
  (`nutils_hook.dll`) into the designated app — and *only* that app — using
  `SetWindowsHookEx`, the same documented mechanism screen readers use. From
  inside the app, each new window is made transparent before it is ever painted,
  so nothing flashes. This is the *polite* form of injection (a hook DLL), **not**
  the `CreateRemoteThread`/memory-writing kind malware uses, and it never touches
  any program you haven't designated. If `nutils_hook.dll` isn't present, NUtils
  falls back to the watcher above.

## Settings

Open **Settings…** from the tray icon to launch the settings editor
(`nutils-settings.exe`), a native wxWidgets dialog chosen for its excellent screen
reader support. It has two tabs:

- **General** — whether stacks are announced by beeping, and the **feedback
  mode**: *Beeps*, *Spoken text*, or *Both*. Spoken text speaks through your screen
  reader (NVDA) when it's running, and falls back to Windows SAPI otherwise (see
  [docs/BUILDING.md](docs/BUILDING.md)).
- **Keybindings** — a list of every shortcut ("action: binding"). Select one and
  press **Set Shortcut…** (or Enter / double-click) to open a capture dialog:
  a checkable **Modifiers** list (Ctrl, Shift, Windows, Alt), and a **Key** field
  that detects the key you press — letters, digits, +, arrows, F-keys, and more.
  Dialog keys like Tab, Enter, Escape and Space are intentionally *not* bindable. A
  live region speaks the detected combination as you build it. The main tab also
  has **Clear**, **Reset to Default**, and **Reset All**.

Click **Save** and the running NUtils picks up the changes within about a second
(it watches `config.toml` for changes) — no restart needed. The editor is a
separate program, so wxWidgets is only loaded when you actually open settings; the
always-running core stays a ~0.5 MB native process.

## Configuration

Configuration is a single `config.toml` at `%APPDATA%\NUtils\config.toml`, created
with defaults on first run. If legacy `hotkeys.ini` / `settings.ini` /
`WinMurderer.ini` files are found next to the executable on first run, they are
migrated automatically. See the sample [`config.toml`](config.toml) for every
option, including hotkey syntax, the WinMurderer `[[rules]]` list, and
`[[managed_apps]]`.

## Building

See [docs/BUILDING.md](docs/BUILDING.md). In short: install Rust (MSVC toolchain)
and run `cargo build --release`.

## A note on antivirus

Window-hiding utilities and AutoIt-packed executables are common sources of
antivirus *false positives*. Version 4 is built to minimize that: native compiled
code, only documented Win32 APIs, and no packing or obfuscation.

The one thing to be aware of: the flash-free auto-hide loads `nutils_hook.dll`
into the app you designate via `SetWindowsHookEx` — a documented hook DLL, the
same mechanism screen readers use, **not** remote-thread or memory-writing
injection. Loading a DLL into another process can still draw attention from
behavior-based antivirus, especially for an *unsigned* binary. **Code-signing
`nutils.exe` and `nutils_hook.dll`** is therefore strongly recommended for any
distribution; a signed hook DLL from a known publisher is treated the same way a
signed screen reader is. If you would rather avoid injection entirely, delete
`nutils_hook.dll` — NUtils falls back to the cross-process watcher (which may
flash for a frame).

## License

NUtils is free software under the **GNU General Public License v3 or later**.
Copyright © 2008–2010 Arbalon, Niko Carpenter, and Tyler Spivey; Rust rewrite 2026.
The full GPL text and the original authors' license notice are in the Git history
(the AutoIt-era `license.txt`).
