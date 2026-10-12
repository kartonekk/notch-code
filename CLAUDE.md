# Notch Code

Desktop app (Tauri 2: Rust + React) for live, local meeting transcription (whisper.cpp / Vulkan). Code lives in `app/`.

## Rules

- **Never run any tests that play speech / audio** (TTS, `System.Speech`, playing recordings, etc.). Nothing may speak or play through the speakers.
- The user tests the app themselves (recording, transcription). Do not launch the app or run builds without asking.

## Building

- Environment: `. .\app\scripts\dev-env.ps1` (MSVC + Ninja + Vulkan SDK, fixes Polish characters in paths).
- Build output goes to `S:\tkb` (short path — whisper.cpp exceeds the 260-character path limit).
- Release: `powershell -ExecutionPolicy Bypass -File .\app\scripts\build-release.ps1` → single file `release/NotchCode.exe` (in `.gitignore`), no installer. Static CRT (`app/src-tauri/.cargo/config.toml`).
- The app must stay a single exe: everything it needs (the Whisper model) is downloaded by the app itself on startup, with progress shown in the notch.
