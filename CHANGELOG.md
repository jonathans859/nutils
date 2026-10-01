# Changelog

Each release's section here becomes its release notes on GitHub, which the
update dialog shows. Start a new release with a `## <version>` heading.

## 4.0.1

- With the visual check on, Electron apps such as Joplin can now be made
  transparent on the first try. They stay on screen for a few frames after the
  change, and NUtils used to judge too early, report "stays visible" and undo
  it; it now gives a window up to 300 ms to disappear.
- Speech now goes through [prismer](https://github.com/trypsynth/prismer), the
  maintained Rust bindings to Prism, with an up-to-date Prism underneath.

## 4.0.0

The first release of NUtils 4, a complete rewrite of NUtils in Rust on the
native Windows API. It keeps what NUtils did and adds a good deal:

- Hide windows into slots and stacks, and bring them back, from hotkeys or the
  tray menu. Focus returns to where you were in the window.
- Make windows transparent: gone from the screen, still usable with a screen
  reader. NUtils checks that it really worked and tells you if not, and can
  optionally confirm it on screen.
- Auto-transparent apps: every window an app opens is made transparent before
  it appears.
- Speak the hidden-window status, optionally naming every hidden window.
- One base modifier (Shift+Alt) for every shortcut, each changeable in an
  accessible settings editor.
- Hidden windows are remembered across a restart of NUtils, and found again
  even if NUtils' state file is lost.
- Speech through NVDA, JAWS, ZoomText or Narrator, beeps, or both; optional
  sound packs.
- WinMurderer, window titles, killing hung apps and process priorities, as
  before.
- Updates: NUtils checks for new releases at startup (you can turn this off)
  and installs them from the tray.
- Portable: everything stays in its own folder.
