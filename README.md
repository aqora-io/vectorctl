# vectorctl

> A simple tool to manage any vector database

[![Rust](https://img.shields.io/badge/rust-1.97.0%2B-green.svg?maxAge=3600)](https://github.com/aqora-io/vectorctl)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)]("./LICENSE")
[![GitHub Release](https://img.shields.io/github/release/tterb/PlayMusic.svg?style=flat)]()

# Introduction

`vectorctl` is a tool designed to simply interact
with vector databases in the same way we do
with relational databases. The design is
highly inspired by [sea-orm](https://github.com/SeaQL/sea-orm)
and [SQLAlchemy](https://github.com/sqlalchemy/sqlalchemy).

The goal of `vectorctl` is to provide a set of tools
to interact with and manage a vectorial database from code
and from a cli in a replicated manner. We believe that
operations over a vectorial database should be replicable
over different environments, and the set of tools provided
should allow that.

From code this is represented as the `migration` internal
crate which allows `vectorctl`'s users to create/update/delete
collections from a vector database.
From cli, to run those migration commands easily in development
and spot anything wrong before running the script in production.

# Support

`vectorctl` supports qdrant only at the moment, but you're
free to add support for other vector databases.

# Installation

```sh
cargo install --git https://github.com/aqora-io/vectorctl --tag v0.2.1 vectorctl-cli --locked
```

# Usage

- init the migration crate (default crate name: migration)

```sh
vectorctl migrate init
```

- create a migration

```sh
vectorctl migrate generate "<MIGRATION_NAME>"
```

# License

`vectorctl` is distributed under the [MIT license](https://www.opensource.org/licenses/mit-license.php)
