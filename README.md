# Starfield datasources — development has moved

This repository has joined **[OrbitalCommons/starfield](https://github.com/OrbitalCommons/starfield)**.
Code, issues, pull requests, CI, and releases now belong to that workspace.

For new projects, use its facade:

```toml
starfield = { version = "0.17", features = ["catalogs", "jpl", "surfaces"] }
```

Individual datasource features are also available. The existing
`starfield-datasources` compatibility crate is now released from the unified
workspace with the same version as the other Starfield crates.

See the [migration guide](https://github.com/OrbitalCommons/starfield/blob/main/docs/workspace-migration.md)
for API changes, feature names, and the transferred issue list.
Please file new issues and pull requests in **Starfield**.

The final source cutover is `0711deff6760b8be96f7bfab35329d76ae6027fe`, including
SFEMv4 albedo conventions and V-band normalization. Its original history is
also preserved in Starfield under the `datasources-history/0711def` tag.
This repository remains available for historical revision pins and review
threads; existing issue links redirect to their transferred Starfield issues.

`starfield-datastore` remains an independent repository and release family.
