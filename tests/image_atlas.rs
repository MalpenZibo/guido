#![cfg(feature = "testing")]
//! Where an image lands on an atlas page, read back in pixels (#546).
//!
//! The goldens show the atlas draws what a texture of its own drew, but only
//! for the images they happen to hold: icons small enough that a page is never
//! close to full, and never cropped. These are the cases they leave out.

mod common;

use guido::prelude::*;

fn solid(width: u32, height: u32, rgba: [u8; 4]) -> ImageSource {
    ImageSource::Rgba {
        width,
        height,
        pixels: rgba.repeat((width * height) as usize).into(),
    }
}

fn boxed(source: ImageSource, width: f32, height: f32, fit: ContentFit) -> Container {
    container()
        .width(width)
        .height(height)
        .child(image(source).content_fit(fit))
}

/// Sixteen entries of 254×254 are a page exactly: 256 with the gutter, four
/// by four on 1024. Allocated any larger they would need a second page;
/// allocated any smaller than they are written, each would be drawn over by
/// its neighbour, and the rows it lost would show at its edge.
#[test]
fn sixteen_entries_the_size_of_a_quarter_page_fill_one_page_and_keep_their_edges() {
    let Some(mut app) = common::headless() else {
        return;
    };
    const SIDE: u32 = 254;
    let colour = |i: u32| [(i * 16) as u8, 255 - (i * 16) as u8, (i * 5) as u8, 255];
    let surface = app.surface(SurfaceConfig::new().height(4 * SIDE), move || {
        container()
            .layout(Flex::column())
            .children((0..4).map(move |row| {
                container()
                    .layout(Flex::row())
                    .children((0..4).map(move |column| {
                        let i = row * 4 + column;
                        boxed(
                            solid(SIDE, SIDE, colour(i)),
                            SIDE as f32,
                            SIDE as f32,
                            ContentFit::Fill,
                        )
                    }))
            }))
    });
    app.configure(surface, 4 * SIDE, 4 * SIDE, 1.0);
    app.step();

    assert_eq!(app.image_textures(), 16);
    assert_eq!(
        app.image_atlas_pages(),
        1,
        "sixteen quarter-page entries are one page"
    );

    let mut centres = Vec::new();
    for i in 0..16 {
        let (x, y) = ((i % 4) * SIDE, (i / 4) * SIDE);
        let centre = app.read_pixel(surface, x + SIDE / 2, y + SIDE / 2);
        for (edge_x, edge_y) in [
            (x + SIDE / 2, y),
            (x + SIDE / 2, y + SIDE - 1),
            (x, y + SIDE / 2),
            (x + SIDE - 1, y + SIDE / 2),
        ] {
            assert_eq!(
                app.read_pixel(surface, edge_x, edge_y),
                centre,
                "image {i} is its own colour at its edge ({edge_x}, {edge_y})"
            );
        }
        centres.push(centre);
    }
    centres.sort();
    centres.dedup();
    assert_eq!(centres.len(), 16, "every image drew its own colour");
}

/// An image goes on a page only if it fits on one both ways: a long thin
/// image keeps a texture of its own, however thin.
#[test]
fn an_image_too_wide_or_too_tall_for_an_entry_keeps_a_texture_of_its_own() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let surface = app.surface(SurfaceConfig::new().height(20), || {
        container()
            .layout(Flex::row())
            .child(boxed(
                solid(300, 10, [255, 0, 0, 255]),
                30.0,
                10.0,
                ContentFit::Fill,
            ))
            .child(boxed(
                solid(10, 300, [0, 255, 0, 255]),
                10.0,
                20.0,
                ContentFit::Fill,
            ))
    });
    app.configure(surface, 40, 20, 1.0);
    app.step();

    assert_eq!(app.image_textures(), 2);
    assert_eq!(app.image_atlas_pages(), 0);
}

/// Four columns or rows of green between two of red at each end.
fn banded(width: u32, height: u32) -> ImageSource {
    let (red, green) = ([255, 0, 0, 255], [0, 255, 0, 255]);
    let pixels = (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                let along = if width > height { x } else { y };
                if (2..6).contains(&along) { green } else { red }
            })
        })
        .collect::<Vec<u8>>();
    ImageSource::Rgba {
        width,
        height,
        pixels: pixels.into(),
    }
}

/// `Cover` crops from the middle of an image, so its texture coordinates start
/// part-way into the entry rather than at its corner. The crop is taken from
/// the entry, not from the page around it: an 8×4 image covering a square
/// shows only its green middle, and so does a 4×8 one the other way. The
/// outermost pixel blends a quarter of the red band in, as bilinear sampling
/// does on a texture of its own too, so the reads are one pixel in.
#[test]
fn a_cropped_image_is_cropped_from_its_own_entry() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let surface = app.surface(SurfaceConfig::new().height(8), || {
        container()
            .layout(Flex::row())
            .child(boxed(banded(8, 4), 8.0, 8.0, ContentFit::Cover))
            .child(boxed(banded(4, 8), 8.0, 8.0, ContentFit::Cover))
    });
    app.configure(surface, 16, 8, 1.0);
    app.step();
    assert_eq!(app.image_atlas_pages(), 1, "both are on the page");

    let green = [0, 255, 0, 255];
    for (x, y) in [(1, 4), (6, 4), (4, 1), (4, 6)] {
        assert_eq!(
            app.read_pixel(surface, x, y),
            green,
            "wide image at ({x}, {y})"
        );
        assert_eq!(
            app.read_pixel(surface, 8 + x, y),
            green,
            "tall image at ({}, {y})",
            8 + x
        );
    }
}

/// The same edges one shelf below another. etagere rounds a shelf up to a
/// multiple of 8, 16, 32 or 64 rows, which hides an entry allocated a little
/// short at 254: it gets the same 256-row shelf either way. At 24 rows it does
/// not: padded to 26, an entry gets a shelf of 32, and allocated a few rows
/// short it would get one of 24, with the next shelf written over its last
/// row. Eighty are two shelves of a 1024-wide page.
#[test]
fn an_entry_on_the_shelf_above_another_keeps_its_last_row() {
    let Some(mut app) = common::headless() else {
        return;
    };
    const SIDE: u32 = 24;
    const COLUMNS: u32 = 10;
    const ROWS: u32 = 8;
    let colour = |i: u32| [(i * 3) as u8, 255 - (i * 3) as u8, 128, 255];
    let surface = app.surface(SurfaceConfig::new().height(ROWS * SIDE), move || {
        container()
            .layout(Flex::column())
            .children((0..ROWS).map(move |row| {
                container()
                    .layout(Flex::row())
                    .children((0..COLUMNS).map(move |column| {
                        boxed(
                            solid(SIDE, SIDE, colour(row * COLUMNS + column)),
                            SIDE as f32,
                            SIDE as f32,
                            ContentFit::Fill,
                        )
                    }))
            }))
    });
    app.configure(surface, COLUMNS * SIDE, ROWS * SIDE, 1.0);
    app.step();
    assert_eq!(app.image_atlas_pages(), 1);

    for i in 0..COLUMNS * ROWS {
        let (x, y) = ((i % COLUMNS) * SIDE, (i / COLUMNS) * SIDE);
        let centre = app.read_pixel(surface, x + SIDE / 2, y + SIDE / 2);
        for (edge_x, edge_y) in [(x + SIDE / 2, y), (x + SIDE / 2, y + SIDE - 1)] {
            assert_eq!(
                app.read_pixel(surface, edge_x, edge_y),
                centre,
                "image {i} is its own colour at ({edge_x}, {edge_y})"
            );
        }
    }
}
