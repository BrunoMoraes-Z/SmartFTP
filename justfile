# SmartFTP tasks. Requires `just` and PowerShell 7 (pwsh); no extra modules.
# List everything with `just` or `just --list`.

set windows-shell := ["pwsh.exe", "-NoLogo", "-NoProfile", "-NonInteractive", "-Command"]
set shell := ["pwsh", "-NoLogo", "-NoProfile", "-NonInteractive", "-Command"]

exe := if os() == "windows" { "smartftp.exe" } else { "smartftp" }

# Show the available recipes
default:
    @just --list --unsorted

# Build the release binary and copy it (plus README, LICENSE, themes) into ./dist
build:
    $ErrorActionPreference = 'Stop'; \
    cargo build --release --locked; \
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; \
    $version = (cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages[0].version; \
    if (Test-Path dist) { Remove-Item dist -Recurse -Force }; \
    New-Item -ItemType Directory dist | Out-Null; \
    Copy-Item "target/release/{{exe}}" "dist/{{exe}}"; \
    Copy-Item README.md dist/; \
    if (Test-Path LICENSE) { Copy-Item LICENSE dist/ }; \
    Copy-Item themes dist/themes -Recurse; \
    $hash = (Get-FileHash "dist/{{exe}}" -Algorithm SHA256).Hash.ToLower(); \
    Write-Host "SmartFTP $version -> dist/{{exe}}"; \
    Write-Host "sha256 $hash"

# Build ./dist and zip it as dist/SmartFTP-<version>-<os>.zip
package: build
    $ErrorActionPreference = 'Stop'; \
    $version = (cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages[0].version; \
    $zip = "dist/SmartFTP-$version-{{os()}}-x86_64.zip"; \
    Compress-Archive -Path dist/* -DestinationPath $zip -Force; \
    Write-Host "packaged $zip"

# Run the app (debug build)
run:
    cargo run

# Run the app with a throwaway config folder (does not touch your saved sites)
run-sandbox:
    $env:SMARTFTP_HOME = Join-Path ([IO.Path]::GetTempPath()) 'smartftp-sandbox'; \
    Write-Host "config: $env:SMARTFTP_HOME"; \
    cargo run

# Start the headless MCP server on stdio
mcp:
    cargo run --release -- --mcp-stdio

# Unit tests
test:
    cargo test

# Formatting check, clippy and tests (what CI runs)
check:
    cargo fmt --check; \
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; \
    cargo clippy --all-targets -- -D warnings; \
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; \
    cargo test

# Format the code
fmt:
    cargo fmt

# Remove build output and ./dist
clean:
    cargo clean; \
    if (Test-Path dist) { Remove-Item dist -Recurse -Force }

# Print the current version (from Cargo.toml)
version:
    @(cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages[0].version

# Bump the version (patch | minor | major | x.y.z), commit and tag it. Push with `just publish`.
release bump="patch":
    $ErrorActionPreference = 'Stop'; \
    if (git status --porcelain) { throw 'Working tree is not clean; commit or stash first.' }; \
    $current = [version](cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages[0].version; \
    $next = switch ('{{bump}}') { \
        'major' { "$($current.Major + 1).0.0" } \
        'minor' { "$($current.Major).$($current.Minor + 1).0" } \
        'patch' { "$($current.Major).$($current.Minor).$($current.Build + 1)" } \
        default { '{{bump}}' } \
    }; \
    if ($next -notmatch '^\d+\.\d+\.\d+$') { throw "Invalid version '$next'" }; \
    if ([version]$next -le $current) { throw "$next is not newer than $current" }; \
    $toml = Get-Content Cargo.toml -Raw; \
    $toml = [regex]::Replace($toml, '(?m)^version = "[^"]+"', "version = `"$next`"", 1); \
    Set-Content Cargo.toml $toml -NoNewline; \
    cargo update --workspace --offline | Out-Null; \
    git add Cargo.toml Cargo.lock; \
    git commit -m "Release v$next"; \
    git tag -a "v$next" -m "SmartFTP v$next"; \
    Write-Host "Tagged v$next ($current -> $next). Run 'just publish' to push and start the release build."

# Push the current branch and tags; a pushed vX.Y.Z tag triggers the GitHub release workflow
publish:
    git push --follow-tags

# Start local FTP and SFTP test servers in Docker (user test / password test)
servers-up:
    docker run -d --name smartftp-test-sftp -p 2222:22 atmoz/sftp test:test:1001::upload; \
    docker run -d --name smartftp-test-ftp -p 2121:21 -p 21000-21010:21000-21010 -e USERS="test|test" -e ADDRESS=127.0.0.1 -e MIN_PORT=21000 -e MAX_PORT=21010 delfer/alpine-ftp-server; \
    Write-Host 'sftp://test:test@127.0.0.1:2222/upload'; \
    Write-Host 'ftp://test:test@127.0.0.1:2121'

# Stop and remove the test servers
servers-down:
    docker rm -f smartftp-test-sftp smartftp-test-ftp
