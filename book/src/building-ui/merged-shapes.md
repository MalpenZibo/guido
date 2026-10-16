# Merged Shapes

A container can draw the backgrounds of its children as **one** shape. Where two
of them come close, a concave neck grows between them; where they overlap, they
are one outline with no seam. It is the shape a bubble makes as it slides out
from under a pill and drops away.

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let bubble_x = create_signal(0.0f32);
let pill = Color::rgb(0.10, 0.10, 0.12);

container()
    .layout(Flex::row())
    .merge_children(14.0)
    .shadow(Shadow::new((0.0, 10.0), 26.0, 0.0, Color::BLACK.with_alpha(0.5)))
    .child(container().width(160.0).height(36.0).corners(18.0).background(pill))
    .child(
        container()
            .width(34.0)
            .height(34.0)
            .corners(17.0)
            .background(pill)
            .translate(move || (bubble_x.get(), 0.0)),
    )
# ;
# }
```

The number is a distance in logical pixels. Children further apart than that do
not touch each other's outline at all, so one that moves away splits off by
itself; as they close in, the neck forms and thickens. `0.0` is a plain union:
overlapping children merge with no seam, and nothing grows between children that
do not overlap. The distance takes a signal like any other property.

## What merges

Each **direct** child's own box: its fill and its border, with its corners and
wherever its transform puts it. What the children hold — text, icons, images,
nested containers — is drawn over the merged shape as usual.

The children do not need the same colour. Fills blend across a neck, and so do
borders: the border runs along the merged outline rather than around each child,
its width and colour blending from one child's to the other's, and a child
without a border fades the border out along the neck.

The merging container's own `border` and `shadow` belong to the merged shape.
The border is drawn along its outline in place of the children's, and every outer
shadow is cast by the whole of it, neck included. An inset shadow stays inside
the container's own box, and so does its background, if it has one, drawn
underneath as usual. A child's own shadows are drawn beneath the merged shape,
and its inset ones over it, inside the child's box.

## What does not

- A child whose fill is a gradient. It is drawn on its own, as it would be
  without `merge_children`, and a debug build says so once.
- Children after the sixteenth with a background, for the same reason and with
  the same warning: every fragment of the merged shape measures every member, so
  the cost is kept bounded.
- Anything below the direct children.

The neck draws but takes no input: each child answers clicks in its own box.

A container that both scrolls and merges merges its scrollbar track as well: the
track is one of its children.
