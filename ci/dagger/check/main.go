package main

import (
	"context"
	"fmt"
	"strings"

	"dagger/hrungnir-ci/internal/dagger"
)

type HrungnirCi struct{}

const (
	rustImage   = "rust:1.99.1-bookworm@sha256:9181b56c2a21ab1d5e73cffe04fc762426d30aa7fac3c91141dd033f49a7fc8b"
	pythonImage = "python:3.12-slim-bookworm@sha256:e697c81897302a4c4333bade000d98038fbb21eb064f8ee1fd42d02ebcd9d337"
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
		WithExec([]string{"python3", "scripts/test-release-artifact.py"}).
		WithExec([]string{"python3", "scripts/test-ffmpeg-lgpl.py"})
	text, err = python.Stdout(ctx)
	if err != nil {
		return "", fmt.Errorf("packaging policy tests: %w", err)
	}
	out.WriteString(text)

	return out.String(), nil
}

// ReaderUX runs the existing interaction assertions in Chromium through the Python CDP runner.
func (m *HrungnirCi) ReaderUX(ctx context.Context, source *dagger.Directory) (string, error) {
	return readerUXContainer(source).WithExec([]string{"python3", "tests/reader-ux/run.py"}).Stdout(ctx)
}

// ReaderReport exports evidence even on assertion failure; CI verifies result.json separately.
func (m *HrungnirCi) ReaderReport(source *dagger.Directory) *dagger.Directory {
	return readerUXContainer(source).WithExec([]string{"python3", "scripts/reader-ux-report.py"}).Directory("/src/artifacts/reader-ux")
}

func readerUXContainer(source *dagger.Directory) *dagger.Container {
	return dag.Container().From(pythonImage).
		WithExec([]string{"sh", "-ec", "apt-get update && apt-get install -y --no-install-recommends chromium && rm -rf /var/lib/apt/lists/*"}).
		WithDirectory("/src", source).WithWorkdir("/src").
		WithEnvVariable("READER_UX_CONTAINER", "1").
		WithEnvVariable("CHROMIUM_BIN", "/usr/bin/chromium").
		WithExec([]string{"python3", "-m", "pip", "install", "--disable-pip-version-check", "--no-cache-dir", "-r", "tests/reader-ux/requirements.txt"})
}
