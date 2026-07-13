# Copilot instructions for ncgopher

Purpose
- Help Copilot/CLI sessions quickly understand how to build, test, and modify ncgopher.

1) Build, test, lint commands
- Install system deps (Debian/Ubuntu): sudo apt install build-essential pkg-config libssl-dev libncurses-dev libsqlite3-dev
- Full build (debug): cargo build
- Full build (release): cargo build --release or make build
- Single test: cargo test <testname>
  - Example: cargo test parse_gophermap
- Run the binary locally: cargo run -- <args>
- Run with debug log and backtrace: RUST_BACKTRACE=1 cargo run -d error.log
- CI uses: cargo check (see .github/workflows/ci.yml)
- Packaging targets: Makefile provides install/install-bin/install-man targets

2) High-level architecture
- UI layer: ncgopher/src/ui/* (layout, setup, dialogs, statusbar). Uses cursive/pancurses backend.
- Controller: src/controller.rs is the central coordinator. It spawns network threads, marshals updates to the UI via a SenderCursive (cb_sink) and stores runtime state (history, bookmarks, current_url, content).
- Protocol handlers:
  - Gopher: src/gophermap.rs and controller.fetch_url/fetch_binary_url
  - Gemini: src/gemini.rs and controller.fetch_gemini_url
  - Finger: src/finger (implemented in controller.fetch_finger_url)
- Persistence: history and bookmarks saved via rusqlite (src/history.rs, src/bookmarks.rs)
- TLS/certs: rustls, rcgen, ring used for client/server certificate handling. Client identities stored in clientcertificates.rs and certificates.rs.
- Concurrency model: network IO happens in spawned threads; UI updates are executed on the main thread via closure messages sent over crossbeam channels.

3) Key conventions and patterns
- UI updates must be posted via the cb_sink closure pattern. See SenderCursive and client_msg! macro.
- Controller methods often clone Url/strings and send closures to the UI thread; avoid moving non-Send types into threads.
- ItemType enum encodes Gopher item semantics; use ItemType::from_url(&url) to decide rendering and download behavior.
- Downloads: use url_tools::download_filename_from_url to derive filenames. Many functions use OpenOptions::create_new to avoid clobbering files.
- Error handling: code uses many unwrap()/expect() calls; prefer returning Results for library-style changes and limit unwraps in new code.
- Settings: global SETTINGS is a RwLock; read() for access, write() for mutations. See src/settings.rs.

4) Tests and CI notes
- There are no unit/integration tests in the repo by default; CI runs `cargo check` on Ubuntu with the necessary system packages installed.
- When adding tests that touch curses/pancurses, run them in an environment with a terminal or isolate UI code behind testable logic.

5) Files of interest for future Copilot sessions
- src/controller.rs (central logic)
- src/gophermap.rs, src/gemini.rs, src/url_tools.rs (protocol helpers)
- src/ui/* (view composition and dialogs)
- src/history.rs, src/bookmarks.rs (persistence)
- Makefile and .github/workflows/ci.yml (how CI builds/checks the repo)

6) Developer workflow hints (short)
- Reproduce build/install issues by installing system libs listed above and running: cargo install --path .
- Run cargo check early to catch obvious compile-time issues (CI mirror).

7) Where to look for common problems
- TLS/certificate handling (controller::get_tls_client_config and danger::NoCertificateVerification)
- Threading and closure moves to cb_sink (use SenderCursive and clone state explicitly)
- File I/O: OpenOptions::create_new may fail if filename collision occurs; Makefile provides install targets.

8) Relevant existing docs consulted
- README.md (install instructions, features)
- CONTRIBUTING.md (PR/issue expectations)
- Makefile and .github/workflows/ci.yml (build/test automation)

If you'd like, commit this file to a branch and push it to origin now (default: create branch `copilot/add-copilot-instructions`).

-- end of file
