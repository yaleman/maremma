# Repository agent instructions

1. You are not done until `just check` passes. Never hand work back with testing reported as "Not
   run (not requested)"; run the repository validation gate and fix any failures caused by the work.
2. Do the work and verify the actual repository state instead of guessing.
3. Use package managers to manage dependencies instead of editing dependency manifests directly.
4. Keep changes simple and scoped. Refactor to reduce code sprawl, and do not add extensibility or
   backwards compatibility unless explicitly requested.
5. When unrelated working-tree changes exist, confirm before committing.

## Project conventions

- Use Rust. Do not add Python to this project.
- Use `pnpm`, not `npm`.
- Avoid OpenSSL.
- Never put tests or library functions in `main`.
- Use SeaORM for database access and migrations. Never use the SeaORM CLI.
- Use `clap` with its `env` feature for CLI, environment, and configuration parsing.
- Use `serde` for serialization and deserialization.
- Do not use `serde_yaml`; use `yaml-rust` when YAML parsing is required.

## Rust error handling

- Model distinct errors and machine-readable statuses with enums or dedicated structs.
- Make status and behavior decisions by matching typed variants, never display strings or serialized
  messages.
- Convert errors to strings only at external boundaries such as HTTP responses, logs, CLI output, or
  storage formats.
- Do not use `.unwrap()`. Use `.expect("clear message")` only in tests, and handle production errors
  explicitly.

## UI and documentation

- Avoid subtitles unless they add necessary information that is not already conveyed by nearby
  content.
- Use project-relative paths in documentation and comments. Do not include private absolute paths.
