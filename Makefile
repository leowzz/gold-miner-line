.PHONY: dev check test build
dev:
	pnpm tauri dev
check:
	pnpm check
	cargo check --manifest-path src-tauri/Cargo.toml
test:
	pnpm test
	cargo test --manifest-path src-tauri/Cargo.toml
build:
	pnpm tauri build
