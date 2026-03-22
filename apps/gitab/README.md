# giTab

giTab is a browser extension that customizes GitLab tab favicons and adds compact visual labels so you can quickly identify issue, merge request, pipeline, and other GitLab resources.

## Features

- Custom favicon shapes and colors by GitLab resource type.
- Label rendering in the favicon for faster tab scanning.
- Support for `gitlab.com` and self-hosted GitLab instances.
- Popup UI to manage allowed GitLab domains.

## Requirements

- Node.js 20+ (recommended)
- pnpm or bun

## Install

```bash
pnpm install
```

Alternative:

```bash
bun install
```

## Development

Run extension in development mode (Chrome profile managed by WXT):

```bash
pnpm dev
```

Firefox development mode:

```bash
pnpm dev:firefox
```

## Build

Build for Chrome:

```bash
pnpm build
```

Build for Firefox:

```bash
pnpm build:firefox
```

## Package ZIP

```bash
pnpm zip
pnpm zip:firefox
```

## Tests

```bash
pnpm test
```

Watch mode:

```bash
pnpm test:watch
```

## Project Structure

- `src/entrypoints/background.ts`: content-script registration and storage sync.
- `src/entrypoints/gitlab.content.ts`: logic injected into GitLab pages.
- `src/entrypoints/popup/`: popup UI and interactions.
- `src/utils/`: URL parsing, storage helpers, and favicon generation.

## Notes

- The extension keeps `gitlab.com` as a default domain.
- Additional GitLab domains can be added from the popup.
