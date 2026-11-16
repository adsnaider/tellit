# Repository Guidelines

## Project Structure & Module Organization

The TypeScript and Svelte sources live under `src`, split by runtime: `src/cli` handles command parsing and Git/Jujutsu integration, `src/server` hosts the Express diff service, `src/client` renders the Svelte 5 web UI, and shared helpers sit in `src/utils` and `src/types`. Unit and integration tests live next to the code they cover as `*.test.ts`. Built artifacts land in `dist/` after `npm run build`; do not edit them manually. Static assets for the UI reside in `public/`, while automation scripts live in `scripts/`.

## Build, Test, and Development Commands

- `npm install`: install workspace dependencies (Node >=21).
- `npm run dev`: run the local development loop (boots the CLI server and Vite UI with hot reload).
- `npm run build`: generate the CLI bundle (`dist/cli`) and production web assets (`dist/client`).
- `npm test` / `npm run test:watch`: execute the Vitest suite once or in watch mode.
- `npm run check`, `npm run check:fix`, and `npm run format`: apply `svelte-check`, `oxlint` type-aware checks, and `oxfmt` formatting before you commit.

## Coding Style & Naming Conventions

The codebase uses strict TypeScript (`tsconfig.strictest`), Svelte 5 runes (`$state`, `$derived`, `$effect`, `$props`), and 2-space indentation; avoid `any` and prefer explicit types. Import types with `import type` and keep module order consistent; `oxlint` and `svelte-check` enforce unused imports/variables and type safety. Svelte components live under `src/client`; use PascalCase filenames for `.svelte` components, camelCase/kebab-case for utilities, and co-locate UI-specific helpers when practical. Run `npm run format` before submitting to apply `oxfmt` formatting.

## Testing Guidelines

Vitest with the `happy-dom` environment drives unit and integration coverage, and Svelte component tests use `@testing-library/svelte`. Place new tests alongside the implementation using the `name.test.ts` pattern and prefer descriptive `describe` blocks tied to features. Ensure asynchronous flows await their assertions and cover both CLI (`src/cli`) and server (`src/server`) branches when they change.

## Commit & Pull Request Guidelines

Keep commits focused, include context in the body, and ensure `npm run check`, `npm test`, and `npm run build` succeed locally before submitting.
