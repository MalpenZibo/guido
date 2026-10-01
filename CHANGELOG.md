# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.11.1](https://github.com/MalpenZibo/guido/compare/v0.11.0...v0.11.1) - 2026-10-01

### Other

- A weight's line height is read from its own face, and a test says so ([#578](https://github.com/MalpenZibo/guido/pull/578))
- A weight the family does not have is the nearest one it does ([#576](https://github.com/MalpenZibo/guido/pull/576))

## [0.11.0](https://github.com/MalpenZibo/guido/compare/v0.10.0...v0.11.0) - 2026-10-01

### Other

- What a line height is made of has a test that would notice it change ([#574](https://github.com/MalpenZibo/guido/pull/574))
- A text's line height is the font's own unless it declares another ([#573](https://github.com/MalpenZibo/guido/pull/573))
- Text is drawn by glyphon 0.12 on cosmic-text 0.19 and wgpu 30 ([#572](https://github.com/MalpenZibo/guido/pull/572))
- SVGs are rasterized by resvg 0.48 ([#571](https://github.com/MalpenZibo/guido/pull/571))
- The lockfile takes every compatible release, and pollster is on 1.x ([#570](https://github.com/MalpenZibo/guido/pull/570))
- Equal text no longer skips history updates ([#564](https://github.com/MalpenZibo/guido/pull/564))
- Each run of an effect owns what it makes, and the next run disposes it ([#565](https://github.com/MalpenZibo/guido/pull/565))
- A callback can wait on the UI thread and read a signal when it runs ([#563](https://github.com/MalpenZibo/guido/pull/563))
- Grouped undo keeps the original caret and selection ([#553](https://github.com/MalpenZibo/guido/pull/553))
- A wrapped widget answers layout_hints and refresh_paint_bounds as itself ([#560](https://github.com/MalpenZibo/guido/pull/560))
- Textured quads composite premultiplied alpha, as their textures are ([#559](https://github.com/MalpenZibo/guido/pull/559))
- An SVG raster is uploaded with straight alpha, like every other image ([#557](https://github.com/MalpenZibo/guido/pull/557))
- An SVG is rasterized off the frame when it is large, in it when it is an icon ([#554](https://github.com/MalpenZibo/guido/pull/554))
- Fractional SVGs keep their intrinsic geometry ([#541](https://github.com/MalpenZibo/guido/pull/541))
- Small images share an atlas page, and a frame of them allocates nothing ([#550](https://github.com/MalpenZibo/guido/pull/550))
- An image can be tinted, and a new tint is a repaint ([#548](https://github.com/MalpenZibo/guido/pull/548))
- A signal id stays stale across an App restart ([#544](https://github.com/MalpenZibo/guido/pull/544))
- A queued replacement survives the last surface closing ([#536](https://github.com/MalpenZibo/guido/pull/536))
- The labeler runs for pull requests from forks too ([#542](https://github.com/MalpenZibo/guido/pull/542))

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
