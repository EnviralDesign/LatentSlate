# LatentSlate

**From latent space to timeline.**

A local-first generative NLE desktop app supporting a range of generation providers, starting with ComfyUI and your own workflows. By [Enviral Design](https://www.enviral-design.com/).


<p align="center">
<img src="media/app/banner.png" alt="LatentSlate banner" width="100%">
</p>

<img src="media/screenshots/mainUI.png" alt="LatentSlate banner" width="100%">

<p align="center">
<img src="media/screenshots/new-project.png" alt="LatentSlate banner" width="49%">
<img src="media/screenshots/generation-que2.png" alt="LatentSlate banner" width="49%">
</p>

<img src="media/screenshots/asset-lab.png" alt="Asset Lab" width="100%">
<img src="media/screenshots/provider-builder.png" alt="Provider Builder" width="100%">
<img src="media/screenshots/transformrotatesnap.png" alt="Provider Builder" width="100%">


## The Gap

ComfyUI is excellent for graph-based generation, but it is not a timeline editor. Traditional NLEs are excellent timeline editors, but they are not built around iterative AI generation, prompt/schema inputs, versioned outputs, or workflow-specific provider wiring.

LatentSlate is exploring the missing middle: a fully local, rust native NLE with a streamlined and opinionated UX combined with a completely un opinionated provider system that can support any API, but is focused on ComfyUI as the intended first class pathway. 

The goal is to let you build and test your workflow in ComfyUI, then bring it into LatentSlate exposing only the parameters and settings you need to care about to create, version, and iterate on your vision.

## What It Is Today

- Windows-first Rust/egui desktop app.
- Project-local asset import for images, audio, and video.
- Timeline with video, audio, and marker tracks.
- Preview panel with transform handles, cached thumbnails, audio waveforms, playback, scrubbing, and diagnostics.
- Generative image/video/audio assets with config files and version history.
- ComfyUI Provider Builder for turning API workflow JSON into timeline-editable provider inputs.
- Experimental OpenAI image, xAI image, and xAI video adapters.
- FFmpeg-backed MP4 export with optional audio mixdown.

The accurate status, limitations, and roadmap are in [docs/PROJECT.md](./docs/PROJECT.md).

## ComfyUI First

The main open-source provider path is bring-your-own ComfyUI:

1. Build and test a workflow in ComfyUI.
2. Export it as API JSON.
3. Open the Provider Builder in the app.
4. Pick an output node and expose only the inputs you want on the timeline.
5. Generate versions directly into the project.

See [docs/PROVIDERS.md](./docs/PROVIDERS.md) for setup, manifest behavior, and current adapter limits.

## Try It

There is no installer yet. Current builds are source-first.

```powershell
git clone <repository-url>
cd <repository-folder>

cargo check
cargo build --release

.\scripts\stage-runtime-dlls.ps1 -Profile release
.\target\release\latentslate.exe
```

You will need Rust stable, FFmpeg development/runtime libraries for `ffmpeg-next`, `ffmpeg.exe` on `PATH` for export, and optionally a local ComfyUI instance at `http://127.0.0.1:8188`.

Local runtime state lives under `.latentslate/` in this repository folder. The directory skeleton is tracked, but provider JSONs, encrypted credentials, and caches are ignored so the app stays inspectable without committing private state.

## Documentation

- [Current status, roadmap, and decisions](./docs/PROJECT.md)
- [Architecture overview](./docs/ARCHITECTURE.md)
- [Provider and ComfyUI guide](./docs/PROVIDERS.md)
- [Desktop automation harness](./docs/DESKTOP_TEST_HARNESS.md)
- [Contributing](./CONTRIBUTING.md)
- [Security](./SECURITY.md)

## Contributing

This project is most useful to ComfyUI power users, AI video creators, Rust desktop contributors, and people willing to test rough Windows builds.

Good early contribution areas: ComfyUI workflow compatibility, provider adapters, export validation, tests/CI, platform bring-up, and sanitized demo assets.

Start with [CONTRIBUTING.md](./CONTRIBUTING.md).

## License

MIT. See [LICENSE](./LICENSE). Created by Enviral Design with contributors.
