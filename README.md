# Transkrypt

Transkrypcja spotkań (Webex, Teams, Zoom, przeglądarka…) na żywo — w całości lokalnie, bez chmury.
Aplikacja żyje w „notchu” u góry ekranu: najedź na niego, wybierz aplikację, z której ma nagrywać dźwięk, i gotowe.

- **Dźwięk z jednej aplikacji** — przechwytywanie WASAPI *process loopback* (tylko wybrany program, nie cały system) + opcjonalnie Twój mikrofon (podpis „Ja”).
- **Rozpoznawanie mowy lokalnie** — [whisper.cpp](https://github.com/ggerganov/whisper.cpp) (`large-v3-turbo`, kwantyzacja q5_0) na GPU przez Vulkan.
- **Jeden plik exe** — przy pierwszym uruchomieniu sam pobiera model (~550 MB), postęp widać w notchu.
- **Pauza / wznowienie**, nazwa pliku z tematu spotkania, transkrypt na żywo z przewijaniem.
- Transkrypty zapisują się jako Markdown w `Dokumenty\Transkrypty`.

## Pobieranie

Gotowy `Transkrypt.exe` jest w zakładce **Releases** — każdy push na `main` publikuje nowe wydanie (`build-<numer>`), najnowsze jest oznaczone jako *Latest*.

Wymagania: Windows 10 2004+ / Windows 11, sterownik karty graficznej z obsługą Vulkan (NVIDIA / AMD / Intel), WebView2 (wbudowany w Windows 11).

## Gdzie co jest

| Co | Gdzie |
|---|---|
| Transkrypty | `Dokumenty\Transkrypty` |
| Model, ustawienia (`config.json`), log | `%LOCALAPPDATA%\Transkrypt` |

`config.json`: `language` (`pl`, `en`, `auto`…), `model`, `use_gpu`, `threads`, `live_preview`.

## Budowanie lokalnie

Potrzebne: Node 22+, Rust (stable), Visual Studio Build Tools (C++), CMake, LLVM, Vulkan SDK.

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools Rustlang.Rustup Kitware.CMake LLVM.LLVM KhronosGroup.VulkanSDK
cd app; npm install; cd ..
powershell -ExecutionPolicy Bypass -File .\app\scripts\build-release.ps1   # -> release\Transkrypt.exe
```

Tryb deweloperski: `. .\app\scripts\dev-env.ps1; cd app; npm run tauri dev`.

## CI

`.github/workflows/build.yml` buduje exe na Windowsie przy każdym pushu i PR:

- push na `main` → nowe wydanie w Releases (`build-<numer>`) z `Transkrypt.exe`,
- inne gałęzie i PR → tylko build (exe jako artefakt w Actions),
- tag `v*` (np. `git tag v0.2.0 && git push --tags`) → wydanie wersji z opisem zmian.

## Uwaga prawna

Nagrywanie i transkrypcja rozmów może wymagać zgody ich uczestników (RODO / lokalne prawo). Odpowiedzialność za zgodne z prawem użycie leży po stronie użytkownika.
