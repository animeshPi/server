# Rust Static File Server

A small Rust server that serves static files from the `static/` directory.

**Overview**

- This repository contains a minimal Rust web server (entry: `src/main.rs`) and static HTML files in `static/`.

**Project Structure**

- `Cargo.toml`: Rust manifest and dependencies.
- `src/main.rs`: Server source code / application entrypoint.
- `static/`: Directory containing `index.html`, `login.html`, and other static assets.

**Build**

Run the release build:

```sh
cargo build --release
```

**Run**

Run the server locally (debug):

```sh
cargo run
```

Or run the release binary:

```sh
cargo run --release
```

The server serves files from the `static/` directory by default. Open `http://localhost:7878/` (or the configured port) to view.

**Development**

- Edit `src/main.rs` to change server behavior or routes.
- Static HTML files live in `static/`; update them to change the UI.
