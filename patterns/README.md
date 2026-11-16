# Test Fixtures — NOT the SSOT

These `.lisp` files are **test fixtures** used by `nc-jarvis-ir` and `nc-jarvis-gen` unit tests.

The **authoritative source of truth (SSOT)** for each project's pattern declarations lives inside the target project itself:

```
{project_root}/.missiond/intent.lisp        ← main architecture
{project_root}/.missiond/intent-db.lisp     ← CRUD patterns
{project_root}/.missiond/intent-workers.lisp ← Worker patterns
...etc
```

**Rule**: Forge is a stateless CLI compiler. It must NEVER contain business-specific declarations. These files exist solely to keep tests self-contained.
