# Path Hashing

## Risk

Snapshot row IDs depend on xxHash64 with seed 0 and an 11-character, zero-padded
base62 encoding. A wrong hasher call would create incompatible snapshot IDs.

## Experiment

`experiments/path-hashing` is a Rust mini-project using:

- `twox-hash` `2.1.2`

It calls `twox_hash::XxHash64::with_seed(0)`, writes the UTF-8 path bytes with
`Hasher::write`, finishes with `Hasher::finish`, and encodes the `u64` with the
spec digit alphabet `0-9`, `A-Z`, `a-z`.

## Proved Calls And Values

- `xxh64_seed0("")` is `0xef46db3751d8e999`, the standard empty-input xxHash64
  seed-0 value.
- `docs/readme.txt` encodes to `K5EzsWuLZ04`.
- `docs/notes` encodes to `1pP6ATZM5gH`.
- The parent path `docs` encodes to `H41WPg3SlMv`.
- The root parent sentinel `/` encodes to `JyBskcNRrBK`.

The product should hash the normalized slash path bytes directly. Do not call
Rust's `Hash` trait on a string for snapshot IDs, because that would hash Rust's
type-level representation rather than the spec bytes.
