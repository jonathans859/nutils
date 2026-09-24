# NUtils

NUtils is a hotkey-driven window manager for Windows, built with screen-reader
users in mind. It hides windows, brings them back, and makes them transparent
with a keystroke, so you can keep lots of things open without them cluttering
your taskbar, your Alt+Tab list or your screen.

It was originally written in AutoIt (2008–2010) by **Niko Carpenter** and
**Tyler Spivey** ([original project](https://github.com/n0ot/nutils)). Version 4
is a complete rewrite in Rust on the native Win32 API; its source is at
<https://github.com/jonathans859/nutils>.

## Features

- **Hide and unhide windows in slots.** Each slot has its own hotkey: press it
  once to hide the active window there, press it again to bring the window back.
  Another hotkey hides the window in the first free slot.
- **Stacks.** Extra sets of slots, for when you need more room or want to keep
  groups of windows apart.
- **Transparent windows.** A transparent window can't be seen on screen, but your
  screen reader can still read it and you can still use it. NUtils checks that
  it really worked. If Windows refuses, or an app makes itself visible again and
  NUtils can't make it transparent again, NUtils tells you. Turn on *Visual
  check* to also have it look at the screen itself. This needs Screen Curtain
  off.
- **Auto-transparent apps.** Choose an app and all of its windows become
  transparent, including dialogs and any new windows it opens, without flashing
  on screen first. Turn it off again and its windows become visible.
- **Hidden-window status.** A hotkey tells you how many windows are hidden and in
  how many stacks. With *Detailed status* turned on, it also reads out every
  hidden window by stack, slot and title.
- **Tray menu.** Lists every hidden window, grouped by stack, so you can click one
  to bring it back.
- **Rename a window** so you can tell apart windows that have the same title.
- **Kill the active window's process** when an app stops responding.
- **Set the active process's priority**, from low up to realtime.
- **WinMurderer.** Automatically close or kill windows that match a watch-list,
  such as a nagging popup.
- **Feedback your way.** Beeps, speech, or both. Speech goes through your screen
  reader (NVDA, JAWS, ZoomText or Narrator), or through Windows speech if no
  screen reader is running. You can also swap the beeps for your own WAV sound
  pack.
- **Remembers hidden windows** if NUtils is restarted. They are forgotten after
  a reboot.
- **Updates.** NUtils checks for a new release when it starts. If there is one,
  the tray icon says so and the tray menu offers to install it; NUtils then
  restarts by itself. You can also choose **Check for updates** in the tray menu
  at any time. Updates are signed, and NUtils refuses one whose signature
  doesn't match.
- **Portable.** Settings are kept in the program's own folder.

## Installing

Unzip the release anywhere, for example into a folder in your user profile or on
a USB stick, and run `nutils.exe`. The zip contains:

| File | Purpose |
|---|---|
| `nutils.exe` | NUtils itself. It runs in the system tray. |
| `nutils-settings.exe` | The settings editor, opened from the tray menu. |
| `nutils_hook.dll` | Stops auto-transparent apps flashing on screen. |
| `README.md` | This file. |
| `LICENSE` | The GNU General Public License, which NUtils is under. |
| `THIRD-PARTY-NOTICES.txt` | Licenses of the libraries NUtils is built with. |

Keep all of these in the same folder. To start NUtils when you sign in, put a
shortcut to `nutils.exe` in your Startup folder (type `shell:startup` in the Run
dialog to open it).

## Settings

Choose **Settings...** from the tray menu. This opens a settings editor that
works well with screen readers:

- **General**: the feedback mode (beeps, speech or both), how stacks are
  announced, detailed status, the visual check, and whether to check for
  updates at startup (with a button to check now).
- **Keybindings**: every shortcut and what it does. All shortcuts share one
  **base modifier** (Shift+Alt at first) plus a key, so changing the base moves
  them all. Select a shortcut and press Enter to change its key, give it its own
  modifiers instead of the base, clear it, or reset it to the default.

When you press **Save**, NUtils picks up the new settings straight away. There is
no need to restart it.

NUtils keeps two files next to `nutils.exe`, so keep it in a folder you can
write to (not `Program Files`):

- `config.toml`: your settings, including the WinMurderer rules. You can also
  edit it by hand; see the sample [`config.toml`](config.toml) for every option.
- `state.toml`: what NUtils records as it runs: the auto-transparent apps, and
  the hidden windows (forgotten after a reboot). You don't need to edit it.

To use a sound pack, put WAV files in a `sounds` folder next to `nutils.exe`.

## Good to know

- Slot 0 is the tenth slot. To bring back a window hidden in another stack,
  switch to that stack first.
- The desktop, the taskbar and the Start menu can't be hidden. NUtils also can't
  act on windows that are running as administrator unless NUtils is too.
- A transparent window may confuse screen-reader features that read what's drawn
  on screen, such as screen review or OCR.
- Auto-transparent loads `nutils_hook.dll` into the app you chose, and only that
  app, using a standard Windows hook (the same method screen readers use).
  Antivirus software may flag this. If you would rather it didn't happen, delete
  `nutils_hook.dll`. Auto-transparent still works without it, but a new window
  may flash on screen for a moment first.

## Building

See [docs/BUILDING.md](docs/BUILDING.md).

## License

NUtils is free software: you can redistribute it and/or modify it under the
terms of the **GNU General Public License, version 3 or (at your option) any
later version**. It comes with no warranty. The full license text is in
[`LICENSE`](LICENSE).

- Original NUtils (AutoIt, versions 1–3): Copyright © 2008–2010 Arbalon, Niko
  Carpenter and Tyler Spivey. <https://github.com/n0ot/nutils>
- Version 4 is a modified version of it, rewritten in Rust in 2026 by
  Jonathan Schuster. Source: <https://github.com/jonathans859/nutils>

NUtils is built with other open-source libraries, each under its own license
(MIT, Apache 2.0, MPL 2.0, the wxWindows Library Licence and others). Their
notices are in `THIRD-PARTY-NOTICES.txt`, which comes with every release.
