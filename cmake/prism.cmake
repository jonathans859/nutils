# CMake toolchain fragment for Prism's native build (the `speech` feature).
#
# Point CMAKE_TOOLCHAIN_FILE at this file when building with `--features speech`.
# CMake resolves the path against its own build directory, so it must be
# absolute -- scripts/build-speech.ps1 takes care of that:
#
#   $env:PRISM_STATIC = "1"
#   $env:CMAKE_TOOLCHAIN_FILE = "$PWD/cmake/prism.cmake"
#   cargo build --release -p nutils --features speech
#
# 1. CRT. `.cargo/config.toml` builds nutils with `+crt-static`, so rustc links
#    the *static* MSVC CRT (libcmt). prism-sys pins MultiThreadedDLL because it
#    assumes the stock dynamic-CRT rustc; left alone, the final link fails with
#    unresolved `__imp__wassert` / `__imp__dtest` and friends. Force the static
#    CRT so both halves agree.
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded" CACHE STRING "" FORCE)

# 2. ATL. Prism's SAPI, JAWS, ZoomText and Sense Reader backends include
#    <atlbase.h>, from the Visual Studio "C++ ATL" component (see BUILDING.md).
#    Set NUTILS_PRISM_NO_ATL=1 to skip those four and build the rest: NVDA,
#    UI Automation, OneCore, ZDSR, PC-Talker and Boy PC Reader still work, so an
#    NVDA user gets a working build without installing ATL. What you give up is
#    JAWS and ZoomText support, and Prism's SAPI fallback for machines with no
#    screen reader at all (OneCore covers that case instead).
if(DEFINED ENV{NUTILS_PRISM_NO_ATL})
  message(STATUS "NUtils: NUTILS_PRISM_NO_ATL set - skipping the ATL backends")
  set(PRISM_ENABLE_SAPI_BACKEND OFF CACHE STRING "" FORCE)
  set(PRISM_ENABLE_JAWS_BACKEND OFF CACHE STRING "" FORCE)
  set(PRISM_ENABLE_ZOOM_TEXT_BACKEND OFF CACHE STRING "" FORCE)
  set(PRISM_ENABLE_SENSE_READER_BACKEND OFF CACHE STRING "" FORCE)
endif()
