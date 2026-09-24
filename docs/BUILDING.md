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
nvdaControllerClient64.dll  (default builds only; a Prism build speaks to NVDA on its own)
nutils-settings.exe         (the settings editor, launched from the tray)
sounds\                     (optional WAV sound pack; PC-speaker beeps used if absent)
config.toml                 (the settings; written here on first run)
state.toml                  (auto-transparent apps and hidden windows; written by NUtils)
```

If `nutils_hook.dll` is missing, NUtils still runs and still auto-hides managed
apps' windows via the cross-process watcher — you just lose the zero-flash,
in-process path (new windows may flash for a frame before being hidden).

`nutils-settings.exe` must sit next to `nutils.exe` so the tray "Settings…" item
can find it.

NUtils is portable: `config.toml` (settings, written only by the user and the
settings editor) and `state.toml` (written by NUtils as it runs) always sit
**next to the executable**, so its folder must be writable. There is no
fallback to `%APPDATA%`.

## Spoken feedback

Choose **Feedback: Spoken text** (or Both) in Settings → General to have NUtils
speak each action ("Hidden", "Stack 2", "Transparent", …).

There are two speech backends, and which one you get is a build-time choice.

### Prism (what CI ships) — `--features speech`

Prism implements NVDA's RPC protocol itself, so `nutils.exe` speaks through NVDA
with **no DLL beside it** — a lone exe copied to another machine still talks. It
also covers JAWS, ZoomText, UI Automation, OneCore, ZDSR and PC-Talker. This is
what the CI artifact is built with, and what you want when you distribute a
single file.

```powershell
.\scripts\build-speech.ps1           # all backends
.\scripts\build-speech.ps1 -NoAtl    # skip the four backends that need ATL
```

The script sets what the build needs: `PRISM_STATIC=1` (link Prism in rather
than as a `prism.dll`) and `CMAKE_TOOLCHAIN_FILE` pointing at `cmake/prism.cmake`,
which forces Prism's native build onto the **static** MSVC CRT. That last part
matters: `.cargo/config.toml` builds with `+crt-static` while `prism-sys` pins
`MultiThreadedDLL`, and mixing them fails the final link on unresolved
`__imp__wassert` / `__imp__dtest`.

On top of the base requirements it needs:

- **CMake** (as for the settings crate; libclang is *not* needed — `prism-sys`
  ships pregenerated bindings).
- The Visual Studio **"C++ ATL for latest build tools"** component (provides
  `atlbase.h`, needed by Prism's SAPI, JAWS, ZoomText and Sense Reader
  backends), installable with:
  ```
  "C:\Program Files (x86)\Microsoft Visual Studio\Installer\setup.exe" modify ^
    --installPath "C:\Program Files\Microsoft Visual Studio\2022\Community" ^
    --add Microsoft.VisualStudio.Component.VC.ATL --quiet --norestart
  ```
  (needs administrator rights). Without it, use `-NoAtl`: NVDA, UI Automation,
  OneCore, ZDSR and PC-Talker are still built, and you give up JAWS, ZoomText and
  Prism's SAPI fallback (OneCore covers machines with no screen reader).

To type-check the Prism path without building the native library at all:

```sh
PRISM_SYS_NO_NATIVE=1 cargo check -p nutils --features speech
```

### The default build — NVDA controller client, else SAPI

A plain `cargo build --release` needs no C++ toolchain at all. It loads
`nvdaControllerClient64.dll` at runtime to speak through **NVDA**, and falls back
to the built-in **Windows Speech API (SAPI)** when that DLL is missing or no
screen reader is running.

The catch is that the DLL must sit next to `nutils.exe` — copy the exe alone to
another machine and speech silently drops to SAPI even with NVDA running. The DLL
is NVDA's freely-redistributable controller client; grab it from the NVDA
"controllerClient" package (or copy the one shipped by apps like TeamTalk).

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
