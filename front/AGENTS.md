# Repository Guidelines

## Project Structure & Module Organization
- `src/main.tsx` boots the Vite + React app; `App.tsx` owns top-level routing.
- UI pieces live in `src/components`; reusable hooks belong in `src/hooks`; clients and config helpers sit under `src/services`.
- Domain types stay in `src/types`; media go in `src/audio`; keep shared styles in `src/App.css` and overrides in `src/index.css`.
- Public assets stay in `public/`; keep `index.html` minimal.
- Runtime config comes from `import.meta.env` (see `src/services/config.ts`); keep `.env*` values out of git.
- REST helpers live in `src/services/NodeService.ts`; adjust those endpoint paths as the backend evolves.

## Build, Test, and Development Commands
- `bun install` (or `npm install`) syncs dependencies; commit the resulting `bun.lockb` when it changes.
- `bun run dev` launches the Vite dev server with hot reload at `http://localhost:5173`.
- `bun run build` performs `tsc -b` type checks and emits production assets into `dist/`.
- `bun run lint` enforces ESLint + TypeScript rules; resolve warnings before opening a pull request.
- `bun run preview` serves the built bundle locally to test deployment behavior.

## Coding Style & Naming Conventions
- Use TypeScript with ES modules and React function components; favor hooks over class state.
- Follow Prettier (`.prettierrc`): four-space indentation and 120-character max line length; run `bun x prettier --write .` on larger refactors.
- Components use `PascalCase`, hooks `useCamelCase`, utilities `camelCase.ts`; place component-specific styles or Tailwind classes beside their owners.
- Environment keys must be camel-cased and prefixed with `VITE_` so Vite exposes them to the client.

## Testing Guidelines
- Automated tests are not yet configured; accompany new features with a Vitest + React Testing Library plan, placing specs in `src/__tests__/` or alongside components as `Component.test.tsx`.
- Until the suite is formalized, document manual QA steps in your PR and exercise core flows (player name entry, config fetch failure) via `bun run dev`.
- Target meaningful edge cases and mock async services to keep tests hermetic once the harness lands.

## Commit & Pull Request Guidelines
- Write concise, imperative commit subjects (≤50 chars) with optional scope, e.g., `feat: add chip deck animation`; add bullet bodies for rationale when needed.
- PRs must summarize the change, link issues or tickets, list environment or migration steps (`cp .env.example .env`), and attach UI screenshots or clips when visuals change.
- Call out remaining risks or follow-ups, note any dependency updates, and request focused reviewers for domain-specific code.
