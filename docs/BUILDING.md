# Building NUtils (Rust rewrite)

NUtils 4.x is a native Windows application written in Rust. There is no AutoIt
dependency any more; the old `.au3` sources live in the Git history (before the
v4 rewrite), not in the working tree.

The workspace has three build outputs:

- **`nutils`** — the always-running core (system tray, hotkeys, hiding,
  transparency). Pure Win32 via the `windows` crate; tiny and dependency-light.
- **`nutils_hook.dll`** (crate `nutils-hook`) — a tiny helper DLL that the core
  injects into a *designated* managed app (via `SetWindowsHookEx`, the same
  mechanism screen readers use) so that app's new windows are made transparent
  in-process, before they are ever shown — the zero-flash auto-hide path. It must
  sit next to `nutils.exe`. Pure Win32; no extra toolchain.
- **`nutils-settings`** — the accessible settings editor, built with **wxWidgets**
  via the `wxdragon` crate. Only this crate needs the heavier toolchain below, and
  it is only launched when the user opens Settings.

## Prerequisites

For **both** crates:

- **Rust** (stable) with the MSVC toolchain: install from <https://rustup.rs>.
- The **Windows SDK** / **Visual Studio Build Tools** ("Desktop development with
  C++") for the MSVC linker and the resource compiler.

Additionally, **only for `nutils-settings`** (wxWidgets):

- **CMake** and **Ninja** on `PATH` (wxDragon builds wxWidgets from source).
- **libclang** (for `bindgen`). Point `bindgen` at it via `LIBCLANG_PATH`, e.g.
  install LLVM, or `pip install libclang` and set
  `LIBCLANG_PATH=<...>\site-packages\clang\native`.
- Network access on the first build (wxDragon downloads the wxWidgets source zip;
  it is cached for subsequent builds).

## Build

Build the core (fast, no extra toolchain):

```sh
cargo build --release -p nutils
```

Build the settings editor (needs the wxWidgets prerequisites above):

```sh
set LIBCLANG_PATH=C:\path\to\libclang\dir
cargo build --release -p nutils-settings
```

Or build everything at once with `cargo build --release --workspace`.

`nutils.exe` is a single self-contained ~0.5 MB executable. Thanks to `+crt-static`
(see `.cargo/config.toml`) it has **no** VC++ redistributable dependency — it
imports only core OS DLLs (kernel32, user32, shell32, comctl32, winmm, …).
`nutils-settings.exe` is larger (it statically links wxWidgets).

For a 32-bit core build (matching the old "compile x86" instruction):

```sh
rustup target add i686-pc-windows-msvc
cargo build --release -p nutils --target i686-pc-windows-msvc
```

## Run / test

```sh
cargo test           # unit tests (hotkey parser)
cargo run --release  # run locally
```

## Distribution layout

Ship the executable together with these, in the same folder:

```
nutils.exe
nutils_hook.dll             (the in-process auto-hide helper; MUST sit next to nutils.exe)
nvdaControllerClient64.dll  (optional; enables speaking through NVDA — see Spoken feedback)
nutils-settings.exe         (the settings editor, launched from the tray)
sounds\                     (optional WAV sound pack; PC-speaker beeps used if absent)
config.toml                 (optional sample; the real config is at %APPDATA%\NUtils)
```

If `nutils_hook.dll` is missing, NUtils still runs and still auto-hides managed
apps' windows via the cross-process watcher — you just lose the zero-flash,
in-process path (new windows may flash for a frame before being hidden).

`nutils-settings.exe` must sit next to `nutils.exe` so the tray "Settings…" item
can find it.

At first run NUtils writes a default `config.toml` to `%APPDATA%\NUtils\`, and
migrates any legacy `hotkeys.ini` / `settings.ini` / `WinMurderer.ini` found next
to the executable.

## Spoken feedback

Choose **Feedback: Spoken text** (or Both) in Settings → General to have NUtils
speak each action ("Hidden", "Stack 2", "Transparent", …).

By default, speech goes through the **active screen reader** when one is running:
NUtils loads `nvdaControllerClient64.dll` (ship it next to `nutils.exe`) and speaks
through **NVDA**. This needs no extra build tooling — it's a plain LoadLibrary at
runtime — and it means screen-reader users hear their own voice, not a second one.
The DLL is NVDA's freely-redistributable controller client; grab it from the NVDA
"controllerClient" package (or copy the one shipped by apps like TeamTalk). If it
is absent, or no screen reader is running, NUtils falls back to the built-in
**Windows Speech API (SAPI)** so speech still works (just in the Windows TTS voice).

To route speech through Prism instead (which additionally supports JAWS/ZoomText),
build with the `speech` feature:

```sh
cargo build --release -p nutils --features speech
```

That build additionally needs, on top of the base requirements:

- **CMake** and **libclang** (as for the settings crate).
- The Visual Studio **"C++ ATL for latest build tools"** component (provides
  `atlbase.h`, needed by Prism's JAWS/SAPI/ZoomText backends), installable with:
  ```
  "C:\Program Files (x86)\Microsoft Visual Studio\Installer\setup.exe" modify ^
    --installPath "C:\Program Files\Microsoft Visual Studio\2022\Community" ^
    --add Microsoft.VisualStudio.Component.VC.ATL --quiet --norestart
  ```
  (needs administrator rights).

To type-check the Prism speech path without building the native library
(no ATL needed):

```sh
PRISM_SYS_NO_NATIVE=1 cargo check -p nutils --features speech
```

## Antivirus / distribution notes

The binary is intentionally built the "clean" way to minimize false positives:
native compiled code (not an AutoIt-packed exe), only documented Win32 APIs, no
process injection, no packing or obfuscation. For public distribution, sign the
executable with an Authenticode code-signing certificate:

```sh
signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 ^
  /f your-cert.pfx /p <password> target\release\nutils.exe
```

A signed binary from a known publisher is the single most effective way to keep
a legitimate tool like this off antivirus heuristics.
