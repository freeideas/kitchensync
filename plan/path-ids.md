# Path IDs

## Risk

The snapshot schema requires `id` and `parent_id` to be xxHash64 with seed 0,
encoded as zero-padded base62 with exactly 11 characters. Rust standard library
does not include xxHash64.

## Experiment

`plan/experiments/path-ids` uses `xxhash-rust` `0.8.12` with feature `xxh64`.
It asserts that `xxhash_rust::xxh64::xxh64(bytes, 0)` matches streaming
`Xxh64::new(0).update(...).digest()`, then encodes the `u64` result into this
base62 alphabet:

`0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz`

The experiment proves these values:

- `/` -> `JyBskcNRrBK`
- `docs` -> `H41WPg3SlMv`
- `docs/readme.txt` -> `K5EzsWuLZ04`
- `docs/notes` -> `1pP6ATZM5gH`

## Proven Package

- `xxhash-rust` `0.8.12` with feature `xxh64`

## Notes For Later Code

The base62 encoding is simple enough to keep in product code: repeatedly divide
the `u64` by 62 from the right side of an 11-byte ASCII buffer initialized to
`0`.

