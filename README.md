[![GitHub Release](https://img.shields.io/github/v/release/robitex/tlx)](https://github.com/robitex/tlx/releases)

# TeX Live eXpress (tlx)

**tlx** is a high-performance alternative installer for **TeX Live**, written in Rust.

It is designed to drastically reduce the time required to install and set up TeX Live
distributions by leveraging parallel network downloads, fast decompression pipelines, and
optimized disk I/O.

## 🚧 Project Status 🚧

`tlx` is currently in an experimental phase and is not yet considered production-ready.
Features and internal architectures are subject to breaking changes as development progresses.

## Key Features

- **Parallel Downloads**: Concurrent package fetching via `tokio` and `reqwest` to fully
  saturate available bandwidth.
- **Accelerated Decompression**: Native LZMA/XZ package extraction optimized for multi-core
  processors.
- **Security Verification**: Automated integrity checks and PGP signature verification for 
  TeX Live repositories.
- **Fast `ls-R` Generation**: Automatic and high-speed TeX file database generation upon
  installation completion.
- **Lightweight & Standalone**: Clean, modular CLI binary with no heavy external
  dependencies.

## Installation

### Building from Source (requires Rust)

Ensure you have a recent Rust toolchain installed:

```bash
git clone https://github.com/robitex/tlx.git
cd tlx
cargo build --release
```

## Usage

Simply run the executable with the following command:
```bash
cargo run --release
```

Please note: at the moment, `tlx` installs only the scheme-full of TeX Live and
works only on Windows as it is at an early stage of development.

## License

This project is licensed under the **Mozilla Public License 2.0 (MPL-2.0)**. See the
[LICENSE](LICENSE) file for details.
