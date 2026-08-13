# starfield-datasources

## Versioning

- When adding a new datasource crate, increment the minor version of the `starfield-datasources` facade crate by 1
- Each new datasource crate starts at version 0.1.0

## Project Structure

- Workspace with datasource crates under `crates/`
- Facade crate at `crates/starfield-datasources/` re-exports all datasource crates behind feature flags
- Default branch is `meawoppl/initial-workspace`

## Patterns

- Clients use `reqwest::blocking::Client` with timeouts (30-60s)
- Error handling uses `starfield::{Result, StarfieldError}`
- All dependencies come from workspace
- Integration tests requiring network are marked `#[ignore]`
- Real data excerpts should be used for parser unit tests
