[![Crates.io](https://img.shields.io/crates/v/z7-defmt-rtt)](https://crates.io/crates/z7-defmt-rtt)
[![docs.rs](https://img.shields.io/docsrs/z7-defmt-rtt)](https://docs.rs/z7-defmt-rtt)
[![ci](https://github.com/ustgt-irs/zynq7000-rs/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/ustgt-irs/zynq7000-rs/actions/workflows/ci.yml)

defmt over RTT for the Zynq 7000
========

Send [`defmt`](https://github.com/knurling-rs/defmt) log messages over RTT (Real-Time
Transfer) on the AMD Zynq 7000 SoCs.

This crate is a fork of
[`defmt-rtt`](https://github.com/knurling-rs/defmt/tree/main/firmware/defmt-rtt) v1.3. The
upstream crate assumes that the debugger and the CPU see the same memory content. On the Zynq 7000,
the debugger reads memory directly while the CPU writes through its data cache. This fork

- places the RTT control block and buffer in OCM,
- performs L1 cache maintenance whenever data is exchanged with the host,
- removes the Cortex-M specific `drop-on-contention` mode.

## Usage

Link the crate by importing it somewhere in your application:

```rust
use z7_defmt_rtt as _;
```

The application also needs a `critical-section` implementation, for example the
`critical-section-single-core` feature of `aarch32-cpu`.

The `memory.x` of the application needs an `OCM` region. The
[zedboard examples](../examples/zedboard/memory.x) can be used as a template.

The cache maintenance only covers the L1 cache. This relies on the default MMU table of the
`zynq7000-hal`, which maps OCM as outer non-cacheable. The crate does not work together with the
`first-segment-ddr-attr` feature of the HAL.

## Reading the logs

[`z7-run`](../../host/z7-run) detects the defmt data in the ELF file and prints the decoded logs
after starting the application. Pass `--no-defmt` to disable this.

## Buffer size

The buffer size defaults to 1024 bytes and can be configured with the `DEFMT_RTT_BUFFER_SIZE`
environment variable. It must be a multiple of the 32 byte cache line size. A power of 2 gives the
best performance.

## Blocking mode

The host can put the RTT channel into blocking mode to avoid losing data. If the host disconnects
while the buffer is full, logging and `defmt::flush` then block forever. Enable the
`disable-blocking-mode` feature if losing data is acceptable.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option. This is the same license as the upstream `defmt-rtt` crate.
