# Notch Code

Live meeting transcription (Webex, Teams, Zoom, browser…) — fully local, no cloud.
The app lives in a "notch" at the top of the screen: hover over it, pick the app to record audio from, and you're done.

- **Audio from a single app** — WASAPI *process loopback* capture (only the selected program, not the whole system) + optionally your microphone (labelled "Me").
- **Local speech recognition** — [whisper.cpp](https://github.com/ggerganov/whisper.cpp) (`large-v3-turbo`, q5_0 quantization) on the GPU via Vulkan.
- **Single exe** — downloads the model (~550 MB) on first launch, with progress shown in the notch.
- **Pause / resume**, file name from the meeting topic, live scrolling transcript.
- Transcripts are saved as Markdown in `Documents\Notch Code`.

## Download

A ready-made `NotchCode.exe` is in **Releases** — every push to `main` publishes a new release (`build-<number>`), the newest one marked *Latest*.

Requirements: Windows 10 2004+ / Windows 11, a GPU driver with Vulkan support (NVIDIA / AMD / Intel), WebView2 (built into Windows 11).

## Where things are

| What | Where |
|---|---|
| Transcripts | `Documents\Notch Code` |
| Model, settings (`config.json`), log | `%LOCALAPPDATA%\Notch Code` |

Folders from the app's former name (`%LOCALAPPDATA%\Transkrypt`, `Documents\Transkrypty`) are moved to the new locations automatically on first launch.

## Start with Windows & updates

- **Start with Windows** — on every launch the app registers a Task Scheduler logon task (`Notch Code`) for the current user, so it starts right at sign-in (no startup-app delay, normal priority). Toggle it from the tray menu (**Start with Windows**) or `autostart` in `config.json`. Local dev builds don't register.
- **Updates** — builds from CI carry their build number. Shortly after launch and every 15 minutes the app checks GitHub Releases for a newer `build-<n>`; if there is one, the notch shows **Update available** with **Update** / **Later**. Nothing is downloaded or installed until you click **Update** (disabled while recording). The new exe is checked (size, SHA-256 from GitHub, PE header), swapped in place and the app restarts. Tray: **Check for updates**. Local builds never update.

## Claude Code

On startup the app looks for the Claude Code CLI (`%USERPROFILE%\.local\bin\claude.exe` or `PATH`) and installs it with the official installer if it's missing (log: `%LOCALAPPDATA%\Notch Code\claude-install.log`).
It checks the sign-in (`claude auth status`) at startup, every 30 minutes and when the notch is opened. When Claude needs to be signed in again, a key icon appears in the notch, the bar says *Claude needs sign-in*, and the panel shows a reminder with **Sign in** (opens the browser) — or **Sign-in window** if the background sign-in can't finish.

`config.json`: `language` (`pl`, `en`, `auto`…), `model`, `use_gpu`, `threads`, `live_preview`.

## Building locally

Requires: Node 22+, Rust (stable), Visual Studio Build Tools (C++), CMake, LLVM, Vulkan SDK.

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools Rustlang.Rustup Kitware.CMake LLVM.LLVM KhronosGroup.VulkanSDK
cd app; npm install; cd ..
powershell -ExecutionPolicy Bypass -File .\app\scripts\build-release.ps1   # -> release\NotchCode.exe
```

Dev mode: `. .\app\scripts\dev-env.ps1; cd app; npm run tauri dev`.

## CI

`.github/workflows/build.yml` builds the exe on Windows on every push and PR:

- push to `main` → new release in Releases (`build-<number>`) with `NotchCode.exe`,
- other branches and PRs → build only (exe as an Actions artifact),
- `v*` tag (e.g. `git tag v0.2.0 && git push --tags`) → versioned release with release notes.

## Legal note

Recording and transcribing conversations may require the consent of their participants (GDPR / local law). The user is responsible for lawful use.
