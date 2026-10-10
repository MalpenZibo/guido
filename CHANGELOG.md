# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.12.0](https://github.com/MalpenZibo/guido/compare/v0.11.1...v0.12.0) - 2026-10-10

### Other

- Every way a container draws is a way it is hit, and a test says so ([#663](https://github.com/MalpenZibo/guido/pull/663))
- Tab's reading order is by each box's centre, and a test says so ([#662](https://github.com/MalpenZibo/guido/pull/662))
- Tab moves the keyboard focus to the next widget that takes it ([#661](https://github.com/MalpenZibo/guido/pull/661))
- Every hover leave of a pointer event is heard before any enter ([#658](https://github.com/MalpenZibo/guido/pull/658))
- A click goes to the widget drawn on top ([#656](https://github.com/MalpenZibo/guido/pull/656))
- A clipped instance is built inline, whatever the rest of the crate looks like ([#659](https://github.com/MalpenZibo/guido/pull/659))
- A shadow pulled inside its box reaches nothing past it ([#655](https://github.com/MalpenZibo/guido/pull/655))
- A shadow's blur reaches the shader no lower than a hard edge ([#654](https://github.com/MalpenZibo/guido/pull/654))
- A file or a trait method the documentation names is one the crate has ([#649](https://github.com/MalpenZibo/guido/pull/649))
- Dropping a Headless leaves the background writes of the others alone ([#651](https://github.com/MalpenZibo/guido/pull/651))
- A widget ref's rect is where the widget is drawn ([#648](https://github.com/MalpenZibo/guido/pull/648))
- A component answers the tree as the widget its body built ([#641](https://github.com/MalpenZibo/guido/pull/641))
- A shadow never shrinks the shape that casts it ([#644](https://github.com/MalpenZibo/guido/pull/644))
- A shadow's reach is where the shader stops drawing it ([#639](https://github.com/MalpenZibo/guido/pull/639))
- A container hears a key being released, along the route its press took ([#635](https://github.com/MalpenZibo/guido/pull/635))
- Bézier easing stays on its curve at flat slopes ([#630](https://github.com/MalpenZibo/guido/pull/630))
- Image decode handles share their source key ([#633](https://github.com/MalpenZibo/guido/pull/633))
- A press route lasts until the last button is up ([#628](https://github.com/MalpenZibo/guido/pull/628))
- Images fit the device texture limit ([#612](https://github.com/MalpenZibo/guido/pull/612))
- A text is shaped in the width it was laid out in and drawn at its box's edge ([#627](https://github.com/MalpenZibo/guido/pull/627))
- Children are offered the size their container reports ([#626](https://github.com/MalpenZibo/guido/pull/626))
- A key-down says whether a held key produced it ([#616](https://github.com/MalpenZibo/guido/pull/616))
- A cluster's boundary is the x of its first glyph ([#620](https://github.com/MalpenZibo/guido/pull/620))
- A text can declare a letter spacing, and every path measures and draws with it ([#619](https://github.com/MalpenZibo/guido/pull/619))
- Scrollbar presses stop the previous glide ([#611](https://github.com/MalpenZibo/guido/pull/611))
- Timer cancellation releases captures outside the table borrow ([#609](https://github.com/MalpenZibo/guido/pull/609))
- A texture is found by its source's identity, not by a sample of it ([#607](https://github.com/MalpenZibo/guido/pull/607))
- A named family that is installed is measured in itself ([#605](https://github.com/MalpenZibo/guido/pull/605))
- Disposed services ignore late commands ([#602](https://github.com/MalpenZibo/guido/pull/602))
- Disposed globals return their replacement signal ([#600](https://github.com/MalpenZibo/guido/pull/600))
- Raster files use their headers for format detection ([#595](https://github.com/MalpenZibo/guido/pull/595))
- Every event has a route, and a test says none reaches a widget outside it ([#598](https://github.com/MalpenZibo/guido/pull/598))
- A container and a literal text create no signal ([#594](https://github.com/MalpenZibo/guido/pull/594))
- A scope allocates what it holds the first time it holds anything ([#592](https://github.com/MalpenZibo/guido/pull/592))
- A derived signal keeps its closure in its own slot ([#597](https://github.com/MalpenZibo/guido/pull/597))
- Signals nothing subscribes to cost no subscriber bookkeeping ([#590](https://github.com/MalpenZibo/guido/pull/590))
- A key goes down the focus path, then to the containers listening for keys ([#596](https://github.com/MalpenZibo/guido/pull/596))
- A pointer event with nowhere to narrow to reaches what the pointer record owes ([#593](https://github.com/MalpenZibo/guido/pull/593))
- Timer handles stay stale after application restart ([#585](https://github.com/MalpenZibo/guido/pull/585))
- A positioned event visits the children that can be under it ([#588](https://github.com/MalpenZibo/guido/pull/588))

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
