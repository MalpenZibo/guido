//! A buffer smaller than its surface: [`BufferExtent::Content`] (#911).
//!
//! The layer surface takes the space. It is anchored, sized and reserved as any
//! other, holds the input and blur regions, and draws one transparent pixel,
//! stretched over itself by a viewport. A **synchronized subsurface** draws the
//! content: its buffer is as large as what is drawn, and it is placed inside
//! the layer surface with `wl_subsurface.set_position`.
//!
//! What makes it work is the synchronized mode. The child's commit — the one
//! wgpu's `present` makes — is cached, and applied by the parent's next commit
//! together with the parent's own state: the position the frame placed the
//! buffer at, the regions read off it, a reservation that changed with it. So
//! the platform commits the parent after every present, and a buffer that grows
//! and moves does both in the same frame.
//!
//! Frame callbacks are asked for on the child, whose buffer is what changes:
//! the parent attaches nothing new, and a compositor throttles the callbacks of
//! a surface with nothing new to show. Both surfaces are scaled through
//! viewports: the parent's pixel stretched to the parent's size, the child's
//! buffer to its logical size, so neither sets a buffer scale — which a
//! one-pixel buffer could not be divided by.
//!
//! Three globals, all needed and none useful alone: `wl_subcompositor` for the
//! child, `wp_viewporter` to stretch and size the two, and `wl_shm` for the
//! pixel — a four-byte pool rather than `wp_single_pixel_buffer_v1`, which niri
//! does not offer. A compositor without one of them gets the whole surface
//! drawn, as [`BufferExtent::Surface`] does.
//!
//! [`BufferExtent::Content`]: crate::surface::BufferExtent::Content
//! [`BufferExtent::Surface`]: crate::surface::BufferExtent::Surface

use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::{
    wl_buffer::WlBuffer, wl_shm, wl_shm::WlShm, wl_shm_pool::WlShmPool,
    wl_subcompositor::WlSubcompositor, wl_subsurface::WlSubsurface, wl_surface::WlSurface,
};
use smithay_client_toolkit::reexports::client::{Proxy, QueueHandle, delegate_noop};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};

use super::scaling::SurfaceScaling;
use super::wayland::WaylandState;
use crate::surface::SurfaceId;

/// The three globals a buffer smaller than its surface needs.
pub(crate) struct ContentGlobals {
    subcompositor: WlSubcompositor,
    viewporter: WpViewporter,
    shm: WlShm,
}

impl ContentGlobals {
    /// All three, or `None` with the reason logged.
    pub(super) fn bind(globals: &GlobalList, qh: &QueueHandle<WaylandState>) -> Option<Self> {
        let subcompositor = globals.bind(qh, 1..=1, ()).ok();
        let viewporter = globals.bind(qh, 1..=1, ()).ok();
        let shm = globals.bind(qh, 1..=1, ()).ok();
        match (subcompositor, viewporter, shm) {
            (Some(subcompositor), Some(viewporter), Some(shm)) => Some(Self {
                subcompositor,
                viewporter,
                shm,
            }),
            _ => {
                log::info!(
                    "wl_subcompositor, wp_viewporter or wl_shm not \
                     available - a content-sized buffer will cover the whole surface"
                );
                None
            }
        }
    }
}

/// The child that draws a surface's content, and what stretches the parent.
pub(crate) struct ContentBuffer {
    /// The child's surface, which wgpu presents to.
    pub(crate) surface: WlSurface,
    subsurface: WlSubsurface,
    /// The child's viewport: the logical size its buffer stands for.
    viewport: WpViewport,
    /// The parent's, stretching its pixel over it: the one it already has for
    /// its fractional scale where it has one, since a second viewport on one
    /// surface is a protocol error, and one of its own otherwise.
    stretch: WpViewport,
    /// Whether `stretch` is this buffer's to destroy, rather than the
    /// parent's scaling's.
    owns_stretch: bool,
    /// The transparent pixel the parent shows.
    pixel: WlBuffer,
    /// Whether the pixel has been attached: once, on the first configure.
    attached: bool,
}

impl ContentBuffer {
    /// Declare the logical size the child's next buffer stands for.
    pub(crate) fn set_destination(&self, width: u32, height: u32) {
        self.viewport.set_destination(width as i32, height as i32);
    }

    /// Put the child's buffer at `(x, y)` in the parent, on the parent's next
    /// commit.
    pub(crate) fn place(&self, x: i32, y: i32) {
        self.subsurface.set_position(x, y);
    }

    /// Stretch the parent's pixel over a parent configured at `width` by
    /// `height` — attaching it the first time, so the parent is mapped by the
    /// commit that shows the first frame.
    pub(crate) fn stretch(&mut self, parent: &WlSurface, width: u32, height: u32) {
        self.stretch.set_destination(width as i32, height as i32);
        if !self.attached {
            parent.attach(Some(&self.pixel), 0, 0);
            parent.damage_buffer(0, 0, 1, 1);
            self.attached = true;
        }
    }

    /// Destroy the child before the parent it hangs from.
    pub(crate) fn destroy(self) {
        self.viewport.destroy();
        self.subsurface.destroy();
        self.surface.destroy();
        if self.owns_stretch {
            self.stretch.destroy();
        }
        self.pixel.destroy();
    }
}

/// One transparent pixel in a pool of its own: four zeroed bytes, which
/// `ARGB8888` reads as nothing at all. The pool goes as soon as the buffer is
/// made, and the memory with the buffer.
fn transparent_pixel(shm: &WlShm, qh: &QueueHandle<WaylandState>) -> Option<WlBuffer> {
    use std::os::fd::{AsFd, FromRawFd, OwnedFd};
    // SAFETY: a name with no interior NUL, and flags memfd_create defines.
    let fd = unsafe { libc::memfd_create(c"guido-pixel".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        log::warn!("memfd_create failed: {}", std::io::Error::last_os_error());
        return None;
    }
    // SAFETY: a descriptor memfd_create has just handed over, owned by nobody else.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let file = std::fs::File::from(fd);
    if let Err(e) = file.set_len(4) {
        log::warn!("Failed to size the transparent pixel: {e}");
        return None;
    }
    let pool = shm.create_pool(file.as_fd(), 4, qh, ());
    let pixel = pool.create_buffer(0, 1, 1, 4, wl_shm::Format::Argb8888, qh, ());
    pool.destroy();
    Some(pixel)
}

impl WaylandState {
    /// Give a layer surface a child to draw its content in, where the
    /// compositor can place one, beside the parent's fractional `scaling`.
    pub(super) fn create_content_buffer(
        &mut self,
        id: SurfaceId,
        parent: &WlSurface,
        scaling: Option<&SurfaceScaling>,
    ) -> Option<ContentBuffer> {
        let globals = self.content_globals.as_ref()?;
        let qh = &self.qh;
        let surface = self.compositor_state.create_surface(qh);
        let subsurface = globals
            .subcompositor
            .get_subsurface(&surface, parent, qh, ());
        let viewport = globals.viewporter.get_viewport(&surface, qh, ());
        let (stretch, owns_stretch) = match scaling {
            Some(scaling) => (scaling.viewport.clone(), false),
            None => (globals.viewporter.get_viewport(parent, qh, ()), true),
        };
        let pixel = transparent_pixel(&globals.shm, qh)?;
        // The parent takes the input, in the coordinates the tree is laid out
        // in: the child takes none, so the pointer over what it draws reaches
        // the parent rather than arriving on the child, shifted.
        self.apply_input_region(&surface, Some(&[]));
        // The child's frame callbacks pace the surface, so they are looked up
        // as the surface's.
        self.surface_lookup.insert(surface.id(), id);
        Some(ContentBuffer {
            surface,
            subsurface,
            viewport,
            stretch,
            owns_stretch,
            pixel,
            attached: false,
        })
    }
}

delegate_noop!(WaylandState: ignore WlSubcompositor);
delegate_noop!(WaylandState: ignore WlSubsurface);
delegate_noop!(WaylandState: ignore WlShm);
delegate_noop!(WaylandState: ignore WlShmPool);
delegate_noop!(WaylandState: ignore WlBuffer);
