# Contributing

Thanks for helping improve ContextForge!

1. Read [DEVELOPMENT.md](DEVELOPMENT.md) to build and test locally, and [ARCHITECTURE.md](ARCHITECTURE.md) for the design.
2. Open an issue before large changes so we can agree on the approach.
3. Keep pull requests focused. Every PR must pass `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace`, and in `apps/desktop`: `npm run typecheck`, `npm run lint`, `npm test`, `npm run build`.
4. New provider integrations must be built on documented interfaces and come with contract tests using the provider's
   documented payloads. Record any live verification in `docs/TEST_RESULTS.md` — never claim live support that was not run.
5. Use [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`).
6. Never commit real memory databases, logs, agent configuration or anything containing personal data.

By contributing you agree that your contributions are licensed under the MIT License.
