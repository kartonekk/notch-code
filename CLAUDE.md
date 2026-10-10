# Transkrypt

Aplikacja desktopowa (Tauri 2: Rust + React) do transkrypcji spotkań na żywo, lokalnie (whisper.cpp / Vulkan). Kod w `app/`.

## Zasady

- **Nie rób żadnych testów z odtwarzaniem mowy / dźwięku** (TTS, `System.Speech`, odtwarzanie nagrań itp.). Nic nie może mówić ani grać przez głośniki.
- Testy działania aplikacji (nagrywanie, transkrypcja) robi użytkownik sam. Nie uruchamiaj aplikacji ani buildów bez pytania.

## Budowanie

- Środowisko: `. .\app\scripts\dev-env.ps1` (MSVC + Ninja + Vulkan SDK, naprawia polskie znaki w ścieżkach).
- Pliki kompilacji idą do `S:\tkb` (krótka ścieżka — whisper.cpp przekracza limit 260 znaków).
- Release: `powershell -ExecutionPolicy Bypass -File .\app\scripts\build-release.ps1` → jeden plik `release/Transkrypt.exe` (w `.gitignore`), bez instalatora. Statyczne CRT (`app/src-tauri/.cargo/config.toml`).
- Aplikacja ma być jednym exe: wszystko, czego potrzebuje (model Whisper), pobiera sama przy starcie, z postępem w notchu.
