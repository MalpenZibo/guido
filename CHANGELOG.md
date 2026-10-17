# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.0](https://github.com/MalpenZibo/guido/compare/v0.12.0...v0.13.0) - 2026-10-17

### Other

- A leaving child no longer leads its ZStack's size ([#791](https://github.com/MalpenZibo/guido/pull/791))
- A dynamic child runs in the scope it was written in ([#789](https://github.com/MalpenZibo/guido/pull/789))
- A ZStack offers its children its size rather than holding them to it ([#788](https://github.com/MalpenZibo/guido/pull/788))
- A child keeps its natural size inside a smaller, clipping parent ([#787](https://github.com/MalpenZibo/guido/pull/787))
- A ZStack aligns its children ([#785](https://github.com/MalpenZibo/guido/pull/785))
- A state override can fade a whole control ([#784](https://github.com/MalpenZibo/guido/pull/784))
- An image's tint follows its control's state ([#783](https://github.com/MalpenZibo/guido/pull/783))
- A resize is laid out from the root whatever else the frame has pending ([#777](https://github.com/MalpenZibo/guido/pull/777))
- A text can be centred on its capitals, not on its line box ([#778](https://github.com/MalpenZibo/guido/pull/778))
- A surface's widget tree finds its own SurfaceHandle in its scope ([#776](https://github.com/MalpenZibo/guido/pull/776))
- A container's border takes room, and an outline that does not ([#772](https://github.com/MalpenZibo/guido/pull/772))
- A widget's paint reach is four outsets, one per side ([#771](https://github.com/MalpenZibo/guido/pull/771))
- Backdrops in one group share their render passes ([#770](https://github.com/MalpenZibo/guido/pull/770))
- The arrows, Home and End scroll the scroller nearest the focus ([#769](https://github.com/MalpenZibo/guido/pull/769))
- A container fades as one picture, and can blur what it draws ([#754](https://github.com/MalpenZibo/guido/pull/754))
- A clipping container cuts its children inside its border ([#768](https://github.com/MalpenZibo/guido/pull/768))
- A stroke can be dashed ([#762](https://github.com/MalpenZibo/guido/pull/762))
- A backdrop pass reuses its uniforms and bind groups from frame to frame ([#763](https://github.com/MalpenZibo/guido/pull/763))
- The scroller nearest the focus pages on Page Up and Page Down ([#761](https://github.com/MalpenZibo/guido/pull/761))
- Shapes in one container can merge into one ([#753](https://github.com/MalpenZibo/guido/pull/753))
- Measuring a content-sized surface reads its output as a snapshot ([#760](https://github.com/MalpenZibo/guido/pull/760))
- Page Up, Page Down and the function keys reach widgets ([#748](https://github.com/MalpenZibo/guido/pull/748))
- A double click reaches widgets, and selects a word in a text input ([#746](https://github.com/MalpenZibo/guido/pull/746))
- A container can have an inset shadow, and more than one shadow ([#750](https://github.com/MalpenZibo/guido/pull/750))
- A container moved by layout can slide to its new place ([#751](https://github.com/MalpenZibo/guido/pull/751))
- A widget can draw paths and strokes, and an arc ([#756](https://github.com/MalpenZibo/guido/pull/756))
- Text can be italic and ask for OpenType features ([#749](https://github.com/MalpenZibo/guido/pull/749))
- A widget ref reports the box layout gave its widget, without the transforms ([#743](https://github.com/MalpenZibo/guido/pull/743))
- Keyframes can start after a delay or at a phase ([#744](https://github.com/MalpenZibo/guido/pull/744))

## [0.12.0](https://github.com/MalpenZibo/guido/compare/v0.11.1...v0.12.0) - 2026-10-16

### Other

- A field whose text is replaced keeps its caret in view ([#724](https://github.com/MalpenZibo/guido/pull/724))
- An autofocus field takes the focus from another surface once the keyboard is on its own ([#732](https://github.com/MalpenZibo/guido/pull/732))
- A text input keeps the focus through a keyboard leave, and each surface gets its focus back ([#730](https://github.com/MalpenZibo/guido/pull/730))
- A widget's paint can read the scale of the frame it is painted at ([#729](https://github.com/MalpenZibo/guido/pull/729))
- A key queued behind a callback's write edits the text the callback left ([#727](https://github.com/MalpenZibo/guido/pull/727))
- Switching a container off ends the hover and press of what is inside it ([#728](https://github.com/MalpenZibo/guido/pull/728))
- A padded scroller stops when its content and its padding reach the edge ([#722](https://github.com/MalpenZibo/guido/pull/722))
- A loose cross minimum is a floor for a stretch, never a cap ([#720](https://github.com/MalpenZibo/guido/pull/720))
- A leaving keyed row stays after the rows that stood before it ([#718](https://github.com/MalpenZibo/guido/pull/718))
- A hidden child of a ZStack takes no part in its size ([#719](https://github.com/MalpenZibo/guido/pull/719))
- Hiding a container ends its hover and its press, as disabling it does ([#717](https://github.com/MalpenZibo/guido/pull/717))
- A row whose children do not fit takes the room from those that can give it up ([#715](https://github.com/MalpenZibo/guido/pull/715))
- Nested clips are cut by their exact intersection ([#713](https://github.com/MalpenZibo/guido/pull/713))
- An undeclared scrollbar hover width follows the declared width ([#709](https://github.com/MalpenZibo/guido/pull/709))
- A clipping container keeps a click out of its corners, not just its box ([#705](https://github.com/MalpenZibo/guido/pull/705))
- Nested rounded clips have a benchmark of their own ([#706](https://github.com/MalpenZibo/guido/pull/706))
- A child that fills an unbounded axis is laid out at its content size ([#703](https://github.com/MalpenZibo/guido/pull/703))
- A surface asks for the fixed size it was just given when its other axis turns to content ([#702](https://github.com/MalpenZibo/guido/pull/702))
- A SpaceAround flex with nothing to space places no child at NaN ([#701](https://github.com/MalpenZibo/guido/pull/701))
- A spring exit that begins where it is going leaves at once ([#692](https://github.com/MalpenZibo/guido/pull/692))
- A flex with spacing alignment on an unbounded axis takes its content's size ([#694](https://github.com/MalpenZibo/guido/pull/694))
- A row aligned on the baseline is tall enough for what it moved down ([#695](https://github.com/MalpenZibo/guido/pull/695))
- A flush runs every queued memo before any effect that could read it ([#688](https://github.com/MalpenZibo/guido/pull/688))
- A container that changes its own size lays its parent out again ([#687](https://github.com/MalpenZibo/guido/pull/687))
- A surface that asked to close takes no more of the input queued for it ([#686](https://github.com/MalpenZibo/guido/pull/686))
- A pasted line break never enters a one-line field ([#685](https://github.com/MalpenZibo/guido/pull/685))
- A label that has not changed costs no allocation a frame ([#684](https://github.com/MalpenZibo/guido/pull/684))
- A hasher reached without a map is read for as well as a map ([#683](https://github.com/MalpenZibo/guido/pull/683))
- The surface root is flattened without a cache entry nobody can read ([#682](https://github.com/MalpenZibo/guido/pull/682))
- The glyph atlases forget what they no longer draw ([#681](https://github.com/MalpenZibo/guido/pull/681))
- Tests say the loop wakes for work it can run, not only that it sleeps ([#680](https://github.com/MalpenZibo/guido/pull/680))
- A surface with nothing to draw lets the loop sleep ([#678](https://github.com/MalpenZibo/guido/pull/678))
- The arrows and both deletes step over what is drawn as one character ([#676](https://github.com/MalpenZibo/guido/pull/676))
- A widget inside a hidden container animates nothing and asks for no frame ([#672](https://github.com/MalpenZibo/guido/pull/672))
- A hidden sibling takes no spacing away from a fill() child ([#671](https://github.com/MalpenZibo/guido/pull/671))
- A surface's scale is 1 until the compositor sends one ([#652](https://github.com/MalpenZibo/guido/pull/652))
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
