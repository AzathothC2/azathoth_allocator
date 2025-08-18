# azathoth_allocator

The main allocator in use by the [AzathothC2](https://github.com/AzathothC2/) beacons

> [!WARNING]
> The `multithread` feature is currently broken and may deadlock and/or panic with sizes that are overly large,
> like: `1607423093` bytes

## Installation
* Manually, via `Cargo.toml`: `azathoth_allocator = "0.1.1"`
* Using the `cargo` cli: `cargo add azathoth_allocator`

## Changelog
* 0.1.0: Initial code commit
* 0.1.1: Fixed windows broken import

## License
MIT