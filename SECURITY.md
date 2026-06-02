# Security Policy

LatentSlate is functional alpha software. There are no stable release channels yet, so security reports should target the current `main` branch unless a maintainer says otherwise.

## Responsible Disclosure

Do not open a public issue for a vulnerability that exposes files, credentials, private provider endpoints, or arbitrary code execution paths.

Preferred reporting path:

1. Use GitHub private vulnerability reporting if it is enabled for this repository.
2. If private reporting is not available, open a public issue that says you need a maintainer security contact, but do not include exploit details.

Please include affected commit/version, platform, reproduction steps, impact, and whether the issue requires a malicious project file, media file, provider URL, ComfyUI workflow, or credential.

## Local File Handling

The app imports media into project-local folders and reads/writes project JSON, generated asset folders, export outputs, workflow manifests, and provider config files. Treat project folders as trusted local workspaces.

Risks to keep in mind:

- A project can reference local media paths inside its folder.
- Generated assets and exports may contain private or copyrighted media.
- Provider manifests and workflow paths can reveal local folder structure.
- The app is not a sandbox for untrusted project files.

Do not share project folders publicly without reviewing `project.json`, `generated/**/config.json`, provider references, and media contents.

## Provider URLs And Workflows

ComfyUI providers connect to the configured base URL and submit workflow JSON to that service. Only point the app at ComfyUI instances and provider endpoints you trust.

Provider and workflow risks:

- A ComfyUI workflow may depend on custom nodes with their own code and security model.
- A malicious or unexpected provider URL can receive prompt text, media paths or uploaded inputs, and workflow metadata.
- Workflow manifests bind editor inputs to ComfyUI node IDs. A changed workflow should be re-saved through the Provider Builder before use.
- Custom HTTP provider execution is not implemented yet, but future adapter work must treat URL construction, headers, credentials, and response parsing as security-sensitive.

## Credentials

Cloud provider configs should store credential IDs, not raw API keys. On Windows, app-managed API keys use user-scoped DPAPI protection through the repo-local credential store.

LatentSlate stores local runtime state under `.latentslate/` in the repository folder:

- `.latentslate/providers/` for local provider JSON files.
- `.latentslate/secrets/credentials.json` for encrypted API key records.
- `.latentslate/cache/` for cache and scratch files.

The folder contents are ignored by git except for `.gitkeep` placeholders. Treat `.latentslate/` as private local configuration.

Do not commit:

- API keys.
- Provider JSON files containing secrets.
- Local credential store files.
- Screenshots that reveal keys, account IDs, private provider URLs, or private workflow names.

## FFmpeg And Media Inputs

The app uses FFmpeg-related libraries for media decode and invokes `ffmpeg.exe` for export. Media parsing is a common attack surface.

Recommendations:

- Keep FFmpeg and related runtime DLLs up to date.
- Avoid opening untrusted media files in sensitive environments.
- Reproduce suspected media parser issues with the smallest file possible.
- Report crashes, hangs, or file-read behavior that can be triggered by a crafted media file.

## Scope

Security-sensitive examples include:

- Reading or writing files outside the selected project/export/provider locations.
- Leaking API keys, provider credentials, or local paths unexpectedly.
- Executing unexpected commands or code through project files, workflow manifests, provider URLs, or media metadata.
- Network requests to unintended hosts.
- Persistent denial-of-service from malformed project, workflow, or media files.
