# 00_released-artifacts: Built artifacts exist in `./released/`

## Behavior
After a successful build, the project produces two release artifacts in `./released/`: the library jar and its MCP wrapper jar. Derives from `./aitc/DESIGN.md` §2.4 ("Every library produces exactly two release artifacts: `released/<name>.<ext>` (the library) and `released/<name>_MCP.<ext>`") and `./aitc/MCP-WRAPPER-SPEC.md` §0.

## $REQ_IDs
- `00.1` — `./released/connection-pool.jar` exists after build.
- `00.2` — `./released/connection-pool_MCP.jar` exists after build.

## Notes
File extension `.jar` follows the project's host language (Java, per `./tools/language.txt`).
