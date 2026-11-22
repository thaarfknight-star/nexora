# Nexora Live Translate — Architecture, Setup & Manual Test Plan

Nexora Live Translate captures internal Windows audio via **WASAPI Loopback** (system-wide or per-application process-tree — never the microphone), transcribes speech locally with `whisper.cpp`, and optionally translates stabilized subtitles via an official user-configured provider.

## 1. Speech Recognition (Local Whisper)

All audio capture and speech-to-text run **100% locally** on your machine. Verified models from `ggerganov/whisper.cpp` (pinned commit `5359861c739e955e79d9a303bcbc70fb988958b1`) are verified with fail-closed SHA-256 checksums before use:

| Model | Size | Languages | Adaptive Snapshot Interval | Best For |
| --- | --- | --- | --- | --- |
| `ggml-tiny.en.bin` | ~74 MB | English only | 250 ms | Lowest latency English streams & games |
| `ggml-base.en.bin` | ~141 MB | English only | 350 ms | Higher accuracy English speech |
| `ggml-tiny.bin` | ~74 MB | Multilingual | 250 ms | Fast multilingual speech |
| `ggml-large-v3-turbo-q5_0.bin` | ~547 MB | Multilingual | 750 ms | High-accuracy Persian / multilingual speech |

**Tip:** Whenever you know the spoken language (e.g., `en` or `fa`), select it explicitly instead of `auto`. Avoiding `auto` speeds up Whisper and reduces misdetections.

## 2. Translation Providers & API Keys

By default, Nexora runs in **Local transcription only (no internet)** mode (`translate_provider: "none"`).

To enable live translation, choose one of the official providers in **Settings → Engines → Live Translate**:

1. **Google Cloud Translation API v2** (`https://translation.googleapis.com/language/translate/v2`)
   - Enable the *Cloud Translation API* in [Google Cloud Console](https://console.cloud.google.com/), create an API key (ideally restricted to the Cloud Translation API), and paste it into Nexora Settings.
   - Sent via HTTP header `X-Goog-Api-Key` (never in query parameters).
2. **Gemini API (`gemini-2.5-flash-lite`)** (`https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-lite:generateContent`)
   - Create an API key in [Google AI Studio](https://aistudio.google.com/apikey) and paste it into Nexora Settings.
   - Sent via HTTP header `x-goog-api-key` (never in query parameters).
   - Passes up to 3 prior finalized sentences as context with a strict translation-only system instruction and prompt-injection delimiters (`<transcript>...</transcript>`).
3. **Local Offline Model (`Nexora-MT v1`)**
   - Reserved scaffold (`TranslateProviderId::Local`) for Nexora's upcoming custom offline translator (`nexora-mt-en-fa-v1.nxmt`).

### API Key Security
- Keys are stored exclusively in **Windows Credential Manager** via `keyring` (`windows-native`).
- Keys are **never** written to `settings.json`, **never** logged in diagnostics, and **never** sent back to the WebView frontend (only boolean saved status is exposed).
- Use the **Test Connection** button to verify key validity, quota status, and latency.

## 3. Privacy & Data Usage

- **Audio never leaves your PC.** Only recognized subtitle *text* is sent to a cloud provider, and only after you explicitly consent and select a cloud provider.
- **Gemini API Free Tier Notice:** Under [Google's Gemini API Additional Terms of Service](https://ai.google.dev/gemini-api/terms_preview) (Unpaid Services), content submitted on the unpaid tier may be used to improve Google products and reviewed by human reviewers. Do not use the free tier for confidential, private, or sensitive conversations.

## 4. Dev Evaluation Tool

In debug builds (`cargo tauri dev`), you can evaluate provider quality and latency against `tests/translation-samples.json` via the privileged `evaluate_translation_samples` command (keys are read strictly from OS secure storage; disabled in release builds).

## 5. Manual Windows Test Scenarios

1. **Offline mid-translation:** Start live subtitles with Google Cloud or Gemini, disconnect Wi-Fi/Ethernet while audio plays. Verify the original Whisper transcript continues updating immediately and the subtitle overlay shows the **Offline** badge rather than freezing or silently showing original text as translated.
2. **Invalid API key:** Save a bogus key, click **Test Connection**. Verify a clear `HTTP 401/403` authentication error is returned without exposing the key in UI or logs.
3. **Quota exhausted (HTTP 429):** Use a rate-limited/exhausted key and verify a clear quota/rate-limit error is displayed without infinite retries.
4. **Game music with no speech:** Play instrumental game soundtrack or background hum for 30+ seconds. Verify the adaptive noise-floor VAD gate adapts and does not flood the overlay with `"Thank you."` / `"Thanks for watching."` / repeated-word hallucinations.
5. **Persian & English speech:** Test English speech (`ggml-base.en.bin` or `ggml-tiny.en.bin`, `source_lang = en`, `target_lang = fa`) and Persian speech (`ggml-large-v3-turbo-q5_0.bin`, `source_lang = fa`, `target_lang = en`). Verify RTL/LTR direction and live partial-to-final transitions.
6. **Mid-run provider switch:** Stop or switch provider between `none`, `google_cloud`, and `gemini`, restart subtitles, and verify the new provider takes effect cleanly.
7. **Quitting app while translating:** Start live subtitles, then quit Nexora from the system tray (or trigger an update). Verify `whisper-server.exe` terminates cleanly (check Task Manager) and the app exits without lingering processes.
