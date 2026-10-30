package main

import (
	"context"
	"fmt"
	"strings"

	"dagger/check/internal/dagger"
)

type HrungnirCi struct{}

const (
	rustImage       = "rust:1.99.1-bookworm@sha256:9181b56c2a21ab1d5e73cffe04fc762426d30aa7fac3c91141dd033f49a7fc8b"
	pythonImage     = "python:3.12-slim-bookworm@sha256:e697c81897302a4c4333bade000d98038fbb21eb064f8ee1fd42d02ebcd9d337"
	nodeImage       = "node:22.23.3-bookworm-slim@sha256:3ed98443660ff48413b5af19ecefce3b9d50845a901a43420d47cc63a9fddbe9"
	playwrightImage = "mcr.microsoft.com/playwright:v1.64.1-noble@sha256:47b510dd9230bf0b9bfc9b20102241e577ea7a99334ab4c3ca19f19770e9b8ea"
)

// BuildLintTest runs the Linux Rust, packaging-policy, and syntax checks.
func (m *HrungnirCi) BuildLintTest(ctx context.Context, source *dagger.Directory) (string, error) {
	var out strings.Builder
	rust := dag.Container().From(rustImage).
		WithExec([]string{"sh", "-ec", "apt-get update && apt-get install -y --no-install-recommends build-essential pkg-config libssl-dev libxdo-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev && rm -rf /var/lib/apt/lists/* && rustup component add clippy"}).
		WithDirectory("/src", source).WithWorkdir("/src").
		WithExec([]string{"cargo", "clippy", "--locked", "--all-targets"}).
		WithExec([]string{"cargo", "clippy", "--locked", "--manifest-path", "src-tauri/Cargo.toml", "--all-targets"}).
		WithExec([]string{"cargo", "test", "--locked"})
	text, err := rust.Stdout(ctx)
	if err != nil {
		return "", fmt.Errorf("Rust lint/tests: %w", err)
	}
	out.WriteString(text)

	python := dag.Container().From(pythonImage).
		WithDirectory("/src", source).WithWorkdir("/src").
		WithExec([]string{"python3", "scripts/test-macos-policy.py"}).
		WithExec([]string{"python3", "scripts/test-ffmpeg-lgpl.py"})
	text, err = python.Stdout(ctx)
	if err != nil {
		return "", fmt.Errorf("packaging policy tests: %w", err)
	}
	out.WriteString(text)

	node := dag.Container().From(nodeImage).
		WithDirectory("/src", source).WithWorkdir("/src").
		WithExec([]string{"npm", "ci"}).
		WithExec([]string{"node", "--check", "reader/app.js"})
	text, err = node.Stdout(ctx)
	if err != nil {
		return "", fmt.Errorf("Node install/syntax check: %w", err)
	}
	out.WriteString(text)
	return out.String(), nil
}

// ReaderUX runs the pinned Chromium interaction suite in its official Playwright image.
func (m *HrungnirCi) ReaderUX(ctx context.Context, source *dagger.Directory) (string, error) {
	node := dag.Container().From(nodeImage)
	return dag.Container().From(playwrightImage).
		WithFile("/usr/local/bin/node", node.File("/usr/local/bin/node")).
		WithDirectory("/src", source).WithWorkdir("/src").
		WithExec([]string{"sh", "-ec", "test \"$(node --version)\" = v22.23.3 && npm ci --ignore-scripts && npm install --no-save --package-lock=false --ignore-scripts playwright@1.64.1 && npx playwright install --with-deps chromium && node tests/reader-ux/run.cjs"}).Stdout(ctx)
}
