# Repository Guidelines

## Project Structure & Module Organization
Source lives in `src/`; `src/main.tsx` boots Vite + React and defers routing to `App.tsx`. Group UI under `src/components`, shared hooks under `src/hooks`, and API/config clients under `src/services` (e.g., `NodeService.ts`). Persist domain models in `src/types`, audio in `src/audio`, and global styles in `src/App.css` with overrides in `src/index.css`. Public assets and HTML shells stay in `public/` and `index.html`. Keep runtime configuration in `src/services/config.ts` and consume via `import.meta.env`.

## Build, Test, and Development Commands
Run `bun install` (or `npm install`) to sync dependencies and preserve `bun.lockb`. Use `bun run dev` for the hot-reload dev server at `http://localhost:5173`. `bun run build` executes `tsc -b` and emits production assets to `dist/`. `bun run lint` enforces ESLint + TypeScript rules; resolve warnings before review. Preview the bundled app with `bun run preview`.

## Coding Style & Naming Conventions
Write TypeScript ES modules and functional React components. Follow the repository Prettier config: four-space indentation and a 120-character line limit (`bun x prettier --write .`). Use `PascalCase` for components, `useCamelCase` for hooks, and `camelCase.ts` for utilities. Environment variables must be camel-cased and prefixed with `VITE_` to surface in Vite builds.

## Testing Guidelines
Vitest + React Testing Library are the target stack. Place specs in `src/__tests__/` or alongside components as `Component.test.tsx`. While automated coverage is evolving, accompany features with a manual QA checklist covering flows such as player name entry and config fetch failures. Mock async services to keep tests deterministic when you add them.

## Commit & Pull Request Guidelines
Commit subjects should be imperative and ≤50 characters (e.g., `feat: add chip deck animation`); add short bullet bodies for context as needed. Pull requests must summarize scope, link tickets, list setup steps like `cp .env.example .env`, and include UI screenshots or clips for visual changes. Highlight remaining risks, dependency updates, and call for domain-specific reviewers.

## Security & Configuration Tips
Do not commit `.env*` files or secrets. Rely on `import.meta.env` for runtime config and document expected variables in the PR. Keep REST endpoint paths synchronized with the backend by updating `src/services/NodeService.ts` as APIs evolve.
