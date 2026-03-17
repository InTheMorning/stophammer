# ADR Workflow

This repo keeps architecture decision records in `docs/adr/`.

Use the ADR guidance and tooling curated at:

- https://adr.github.io/
- https://adr.github.io/adr-tooling/

For this repo, use the official Nygard-style
[`adr-tools`](https://github.com/npryce/adr-tools) command-line tool rather
than local custom scripts.

Install guidance from upstream:

- https://github.com/npryce/adr-tools/blob/master/INSTALL.md

Typical workflow:

```bash
adr help
adr list
adr new "Your decision title"
adr new -s 24 "Your replacement decision"
```

Notes for this repo:

- `.adr-dir` points `adr-tools` at `docs/adr/`
- ADRs live in `docs/adr/`, not the upstream default `doc/adr/`
- `docs/adr/templates/template.md` overrides the upstream default so new ADRs
  match this repo's current `# ADR NNNN: ...` plus `## Status` formatting
- the existing ADR set predates this standardization and is slightly mixed in
  formatting
- new ADRs should follow the `adr-tools` / Nygard shape unless we explicitly
  migrate to a different template family later
