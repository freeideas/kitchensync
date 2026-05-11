# 05_events: `on_event` is invoked once per acquire and once per release

## Behavior
When `register_pool` is called with a non-`none` `on_event` callback, that callback is invoked once per `acquire` and once per `release`. Each invocation carries four values: a `kind` (`"acquire"` or `"release"`), the pool's `key`, the count of currently in-use connections after the update, and the pool's `mc`. When `on_event` is `none`, no event callback is invoked. Derives from `./specs/SPEC.md` §"Observation".

## $REQ_IDs
- `05.1` — Each `acquire` invokes `on_event` once with `kind="acquire"`, the pool's `key`, the post-update in-use count, and the pool's `mc`.
- `05.2` — Each `release` invokes `on_event` once with `kind="release"`, the pool's `key`, the post-update in-use count, and the pool's `mc`.
- `05.3` — On `acquire`, the in-use count reported is one greater than the count just before the acquire took its slot.
- `05.4` — On `release`, the in-use count reported is one less than the count just before the release returned its slot.
- `05.5` — When `on_event` is `none`, no event callback is invoked for any `acquire` or `release`.

## Notes
"Post-update" means the count after the slot accounting for that operation has been applied. The `mc` reported is the value retained at first registration.
