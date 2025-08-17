# azathoth_allocator

The main allocator in use by the [AzathothC2](https://github.com/AzathothC2/) beacons

> [!WARNING]
> The `multithread` feature is currently broken and may deadlock and/or panic with sizes that are overly large,
> like: `1607423093` bytes

## Installation
* Manually, via `Cargo.toml`: `azathoth_allocator = "0.1.0"`
* Using the `cargo` cli: `cargo add azathoth_allocator`

## License
MIT