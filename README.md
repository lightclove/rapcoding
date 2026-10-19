# rapcoding

Голосовой ввод под курсор. Хоткей → говоришь → текст в активном окне.

Rust. Windows — основная цель. macOS/Linux — собирается, не проверялось.
cmake/clang/GPU не нужны.

## Принцип

Хоткей (global-hotkey, hold/toggle) → захват микрофона (cpal) → downmix в 16 кГц,
обрезка тишины → ASR (vosk или whisper) → вставка (прямой ввод Unicode или буфер+Ctrl+V).

Бинарь ~600 КБ. Движок и модель — отдельно, вне бинаря.

## Установка

1. `cargo build --release`
2. Движок и модель:
   - vosk (по умолчанию, RU, ~1 с): `libvosk.dll` (vosk-api v0.3.45) рядом с exe,
     `vosk-model-small-ru-0.22` в `%APPDATA%\rapcoding\models\`
   - whisper (фолбэк, RU/EN, ~9 с): `whisper-cli` (whisper.cpp) в PATH,
     `ggml-small-q5_1.bin` в `models\`
3. `rapcoding check`
4. `rapcoding run` — хоткей `Ctrl+Alt+Space`, отпустил — вставилось

## Команды

```
rapcoding run               демон (по умолчанию)
rapcoding check             проверка конфига, микрофона, хоткея, ASR
rapcoding devices           список устройств ввода
rapcoding transcribe x.wav  распознать WAV (16 кГц PCM), вывести текст
rapcoding inject "текст"    тест вставки в активное окно
rapcoding init              записать дефолтный конфиг
```

## Конфиг

`%APPDATA%\rapcoding\config.toml`, создаётся при первом запуске.
Шаблон с комментариями — `assets/config.toml`.

- `asr.backend` — `vosk` | `sidecar` (whisper)
- `asr.model`, `asr.language`, `asr.threads`
- `general.hotkey`, `general.mode` — `hold` | `toggle`
- `inject.method` — `type` (прямой ввод) | `paste` (буфер)
- `text.strip_punctuation` — слова без знаков
- `text.voice_commands` — `off` | `safe` | `aggressive`

## Голосовые команды (safe)

`новая строка` / `new line` — перенос. `открыть скобку` / `open parenthesis` — `(`.
`точка с запятой` — `;`. `двойное двоеточие` / `double colon` — `::`.
Одиночные (`запятая`, `равно`, `слэш`) — режим `aggressive`.

## Ограничения

- Windows: SendInput не пишет в elevated-окна (UIPI)
- macOS: нужен Accessibility; хоткей должен жить на main-потоке (не проверено)
- Linux: только X11, Wayland без глобальных хоткеев

## Сборка под размер

`opt-level = "z"`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`.
