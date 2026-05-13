# 02_reconciliation: Universal per-participant reconciliation after a winner is chosen

## Behavior
Once the decision rules choose an authoritative state, every participant is reconciled to that state. Matching observations get `NoOp`; missing entries get `ReceiveFile` or `CreateDirectory`; observations that do not match the decided state get `Displace`. The decision output also reports the chosen entry kind and the winning entry's metadata. Derived from SPEC.md "Output" and the closing reconciliation paragraph of "Decision rules".

## $REQ_IDs
- `02.9` — A participant whose observation matches the decided `File` state (same `byte_size`, `mod_time` within tolerance) gets `NoOp`.
- `02.10` — A participant whose observation matches the decided `Directory` state (a directory exists at the name) gets `NoOp`.
- `02.11` — When `entry_kind` is `None`, a participant observing `Absent` gets `NoOp`.
- `02.12` — A participant lacking the entry when the decision is `File` gets `ReceiveFile { source = winning_source }`.
- `02.13` — A participant lacking the entry when the decision is `Directory` gets `CreateDirectory`.
- `02.14` — A participant observing `File` that does not match the decided state gets `Displace`.
- `02.15` — When `entry_kind` is `File`, the decision reports `winning_mod_time` from the participant whose observation provided the winning file metadata.
- `02.16` — When `entry_kind` is `File`, the decision reports `winning_byte_size` from the participant whose observation provided the winning file metadata.
- `02.17` — When `entry_kind` is `File`, the decision reports `winning_source` as the participant whose observation provided the winning file metadata.
- `02.18` — When `entry_kind` is `Directory`, the decision reports `winning_mod_time`.
- `02.19` — When `entry_kind` is `Directory`, the decision reports `winning_byte_size` equal to the directory sentinel `-1`.
- `02.20` — When `entry_kind` is `Directory`, the decision omits `winning_source`.
- `02.21` — When `entry_kind` is `None`, the decision omits `winning_mod_time`.
- `02.22` — When `entry_kind` is `None`, the decision omits `winning_byte_size`.
- `02.23` — When `entry_kind` is `None`, the decision omits `winning_source`.
- `02.24` — A participant observing `Directory` that does not match the decided state gets `Displace`.
- `02.25` — The decision reports an action for each participant identifier.
- `02.26` — The decision reports `entry_kind` as `File`, `Directory`, or `None`.
