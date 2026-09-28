# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.10.0](https://github.com/MalpenZibo/guido/compare/v0.9.0...v0.10.0) - 2026-09-26

### Other

- A dropped secret's pages are checked where no other thread maps ([#534](https://github.com/MalpenZibo/guido/pull/534))
- An app between windows holds no GPU device ([#533](https://github.com/MalpenZibo/guido/pull/533))
- The GPU device reserves megabytes, not a quarter of a gigabyte ([#532](https://github.com/MalpenZibo/guido/pull/532))
- An app can outlive its last surface ([#528](https://github.com/MalpenZibo/guido/pull/528))

## [0.9.0](https://github.com/MalpenZibo/guido/compare/v0.8.0...v0.9.0) - 2026-09-24

### Other

- The clipboard paths a test can reach are watched ([#524](https://github.com/MalpenZibo/guido/pull/524))
- Guido's logo is the umarell, and the wordmark's o is his lens ([#517](https://github.com/MalpenZibo/guido/pull/517))
- A clipboard offer is read when something pastes, and what is read is wiped ([#523](https://github.com/MalpenZibo/guido/pull/523))
- A password field keeps its text in a Password, and text_input no longer masks ([#518](https://github.com/MalpenZibo/guido/pull/518))
- guido builds on smithay-client-toolkit 0.21, the line upstream fixes land on ([#521](https://github.com/MalpenZibo/guido/pull/521))

## [0.8.0](https://github.com/MalpenZibo/guido/compare/v0.7.0...v0.8.0) - 2026-09-23

### Other

- Lock surfaces are asked for as soon as the lock is, not once it is granted ([#515](https://github.com/MalpenZibo/guido/pull/515))
- The image texture cache is bounded by bytes, 100 MB unless the application says otherwise ([#513](https://github.com/MalpenZibo/guido/pull/513))
- A raster image is decoded off the frame, and the image says when it is ready ([#511](https://github.com/MalpenZibo/guido/pull/511))
- A release is one pull request, opened and published by release-plz ([#508](https://github.com/MalpenZibo/guido/pull/508))
- The release workflow's input and pull request say what they do in a line each ([#505](https://github.com/MalpenZibo/guido/pull/505))

## [0.7.0](https://github.com/MalpenZibo/guido/releases/tag/v0.7.0) - 2026-09-23

The notes for 0.7.0 and every release before it are on
[GitHub](https://github.com/MalpenZibo/guido/releases).
