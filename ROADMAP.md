# Roadmap

This is the concise public roadmap. The living project status and decisions live in [docs/PROJECT.md](./docs/PROJECT.md).

## Current Status

NLA AI Video Creator is a Windows-first functional alpha. Core project editing, timeline operations, preview, audio playback/waveforms, generative asset versioning, ComfyUI manifest-based providers, experimental cloud provider adapters, and FFmpeg MP4 export are implemented enough for development and early testing.

It is not production-ready and does not have packaged releases yet.

## Near Term

- Harden export with better diagnostics, more format coverage, and broader source-media validation.
- Improve video-generation workflows, especially ComfyUI I2V, T2V, V2V, and first/last-frame patterns.
- Expand provider adapters based on contributor/user demand.
- Improve Provider Builder drift handling when ComfyUI workflows change.
- Add a small sanitized demo project and public screenshots.
- Broaden automated tests around project persistence, provider manifests, generation inputs, export, and timeline operations.
- Keep CI focused on reliable checks first, then add stricter clippy gates once existing lint debt is cleaned up.

## Platform Roadmap

- Windows: primary development platform.
- macOS: future investigation.
- Linux: future investigation.

Before macOS/Linux are considered supported, the app needs platform-specific work for FFmpeg setup, credential storage, file dialogs, audio playback behavior, and release packaging.

## Provider Roadmap

- ComfyUI remains the primary open-source provider path.
- OpenAI image, xAI image, and xAI video adapters exist but need more user-facing validation.
- `CustomHttp` is modeled but not implemented.
- fal.ai, Replicate, Veo, and other hosted providers are future adapter work.

## Not Planned As A Primary Goal

This project is not trying to replace Premiere, Resolve, or a full-featured NLE. The goal is an AI-native planning, generation, timeline, and export tool that can also hand off media to traditional editors when needed.
