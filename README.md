# drupal-index

Fast local CLI indexer for [`drupal/drupal`](https://github.com/drupal/drupal) (`main`), built in Rust with `tree-sitter-php`, `serde_yaml_ng`, and `SQLite` FTS5.

## Features

- Shallow-clones and updates `drupal/drupal` (`main`) in `~/.local/share/drupal-index/repo`.
- Extracts PHP classes, interfaces, traits, enums, methods, functions, `#[Hook('...')]` attributes, and `hook_*` definitions via `tree-sitter-php`.
- Extracts Drupal YAML metadata: `*.services.yml`, `*.routing.yml`, `*.info.yml`, `*.permissions.yml`, `*.libraries.yml`, `*.schema.yml`.
- Stores everything in `~/.local/share/drupal-index/index.sqlite` with FTS5 full-text search.
- After a re-index (or when `vendor/bin/phpcs` is missing) runs `composer update --no-scripts` in the
  mirror with `composer.lock` deleted: core's `require-dev` puts `phpcs` + `drupal/coder` and
  `phpstan` + `phpstan-drupal` into `repo/vendor/bin` (the next sync's `git reset --hard` restores the
  lock). Without `composer` on `PATH` it warns and skips.

## Install

```sh
cargo install --path .
```

## Usage

```sh
drupal-index sync                     # clone/pull main and index if HEAD changed
drupal-index stats                    # show indexed commit SHA and counts
drupal-index symbol EntityTypeManager # find PHP class/interface/trait/method
drupal-index hook entity_presave      # find #[Hook('...')] and hook_*
drupal-index service entity_type.manager
drupal-index route node
drupal-index search "CacheableResponse" --kind class --json
```
