<!-- PROJECT LOGO -->
<br />
<h3 align="center">AtomBlocks</h3>

  <p align="center">
    async, absolutely lightweight and dead simple bar for dwm and similar window managers
    <br />
    <br />
    <a href="https://github.com/milchinskiy/atomblocks/issues">Report Bug</a>
    ·
    <a href="https://github.com/milchinskiy/atomblocks/issues">Request Feature</a>
  </p>
</div>

<!-- ABOUT THE PROJECT -->

## About The Project

another bar implementation for the DWM window manager and similar ones, with asynchronous and independent blocks update.

Each block is supervised independently. A slow or stuck command does not delay updates from other blocks, repeated manual refreshes are coalesced, and missed periodic ticks are skipped rather than replayed in a catch-up burst.

<!-- GETTING STARTED -->

## Getting Started

To get a local copy up and running follow these simple example steps.

### Prerequisites

Install Rust and Cargo. The easiest way to get Cargo is to install the current stable release of Rust by using rustup. Installing Rust using rustup will also install cargo.

- install rustup:

  ```sh
  curl https://sh.rustup.rs -sSf | sh
  ```

- install stable rust and cargo:
  ```sh
  rustup install stable
  ```

### Build from sources

1. Clone the repo
   ```sh
   git clone https://github.com/milchinskiy/atomblocks.git && cd ./atomblocks
   ```
2. Build release
   ```sh
   cargo build --release --locked
   ```

### Install from crates.io

```sh
cargo install atomblocks
```

### Run via Nix Flakes

```sh
nix run github:milchinskiy/atomblocks -- run --config <your.config.toml>
```

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- USAGE EXAMPLES -->

## Usage

An example configuration can be found in [sample/config.toml](sample/config.toml).

Without `--config`, AtomBlocks searches these locations in order and uses the first existing file:

- `$XDG_CONFIG_HOME`/atomblocks/config.toml, when `XDG_CONFIG_HOME` is non-empty and absolute
- `$HOME`/.config/atomblocks/config.toml
- `/etc/atomblocks/config.toml`

An explicit `--config` path is authoritative and is never replaced by discovery fallback.

### Block execution

Blocks are executed through `sh -c`, preserving shell pipelines, quoting, variable expansion, the current working directory, and the inherited environment. Child stdin is `/dev/null`; status commands are noninteractive.

`interval` is optional and measured in seconds. A positive interval runs once immediately and then periodically. `interval = 0` and an omitted interval both mean manual-only. If a periodic deadline occurs while that block is still running, the tick is skipped. Repeated manual hits while a block is running produce at most one immediate follow-up run.

Two optional safety settings are available per block:

```toml
[[block]]
execute = "some-command"
interval = 5
timeout = 2.5
output_limit = 65536
```

`timeout` limits one invocation; it is disabled when omitted. `output_limit` bounds captured stdout and defaults to 64 KiB. Stderr is also drained with a bounded diagnostic buffer. Exceeding the limit, timing out, failing to spawn, or failing to collect a command preserves the block's previous complete value and does not stop unrelated blocks.

A nonzero exit status is reported on stderr with the block index and bounded command stderr, but its stdout is still accepted for compatibility with scripts that intentionally return nonzero after printing useful status text. Repeated identical diagnostics are suppressed until the block's diagnostic changes or clears.

### Run

```sh
atomblocks run
```

By default AtomBlocks writes the complete assembled bar to the X11 root window's `WM_NAME` whenever a block changes.

### Write bar updates to stdout

```sh
atomblocks run --stdout
atomblocks run --stdout --config ./my-custom-config.toml | lemonbar
atomblocks run --stdout --config ./my-custom-config.toml | dzen2
```

With `--stdout`, AtomBlocks writes the complete bar to standard output instead of updating `WM_NAME`. Each update ends with one newline. Carriage returns and newlines within the assembled bar are removed so each update remains one record. Block order, decorations, delimiters, and empty-block filtering are unchanged, and no initial empty update is emitted.

Output is serialized independently from command execution. Pending complete snapshots are coalesced to the latest value if the consumer stalls, so a blocked stdout consumer does not stop block scheduling or create an unbounded output queue. A closed output pipe ends the process successfully; other output failures are reported on stderr and return a nonzero status.

Both output modes still require X11 (`DISPLAY`) because `atomblocks hit <ID>` uses the existing X11 property protocol. `--stdout` does not enable headless operation.

### Manually hit the block to update

```sh
atomblocks hit <ID>
# where <ID> is the zero-based block index in the config file
```

Hit requests are asynchronous. A successful `hit` command confirms delivery to the X server, not completion of the target block. AtomBlocks drains large hit queues incrementally and deduplicates pending IDs. Multiple AtomBlocks instances on the same X screen still share the root-window hit property and, in default mode, `WM_NAME`; instance isolation is not part of the v0.3 protocol.

### Run with custom config

```sh
atomblocks run --config ./my-custom-config.toml
```

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Shutdown and failures

SIGINT and SIGTERM stop new work, terminate each owned command process group, escalate to SIGKILL after a short grace period when necessary, reap direct children, and stop backend threads. This covers ordinary shell pipelines and descendants that remain in the command's process group. Deliberately daemonized descendants that escape that process group are unsupported block behavior.

Loss of the X server is fatal because both normal X11 output and manual-hit control depend on it. AtomBlocks reports the failure and performs the same command cleanup instead of remaining alive without a usable control backend.

## Tests

Run unit tests:

```sh
cargo test --locked
```

The process/X11 tests require an isolated X server. With Xvfb and `xvfb-run` installed:

```sh
xvfb-run -a cargo test --locked --test stdout -- --ignored --test-threads=1
```

These tests modify root-window properties and exercise process cleanup. Do not run them against your desktop X server. They are ignored by default, while CLI, config, scheduler, hit coalescing, rendering, and output-format tests run without X11.

<!-- CONTRIBUTING -->

## Contributing

Contributions are what make the open source community such an amazing place to learn, inspire, and create. Any contributions you make are **greatly appreciated**.

If you have a suggestion that would make this better, please fork the repo and create a pull request. You can also simply open an issue with the tag "enhancement".
Don't forget to give the project a star! Thanks again!

1. Fork the Project
2. Create your Feature Branch (`git checkout -b feature/AmazingFeature`)
3. Commit your Changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to the Branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request

<!-- LICENSE -->

## License

Distributed under the MIT License. See `LICENSE` file for more information.

<!-- MARKDOWN LINKS & IMAGES -->
<!-- https://www.markdownguide.org/basic-syntax/#reference-style-links -->

[contributors-shield]: https://img.shields.io/github/contributors/milchinskiy/atomblocks.svg?style=for-the-badge
[contributors-url]: https://github.com/milchinskiy/atomblocks/graphs/contributors
[forks-shield]: https://img.shields.io/github/forks/milchinskiy/atomblocks.svg?style=for-the-badge
[forks-url]: https://github.com/milchinskiy/atomblocks/network/members
[stars-shield]: https://img.shields.io/github/stars/milchinskiy/atomblocks.svg?style=for-the-badge
[stars-url]: https://github.com/milchinskiy/atomblocks/stargazers
[issues-shield]: https://img.shields.io/github/issues/milchinskiy/atomblocks.svg?style=for-the-badge
[issues-url]: https://github.com/milchinskiy/atomblocks/issues
