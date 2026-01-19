I want create a rust gui library using wgpu and the wulkan renderer.
The primary scope is to create wayland widget using the layer shell protocol.

The library should have just a few component:

- text to display text
- row to show element in a row using a flexbox
- column as the row but for the column
- a box to UI related stuff border, padding, backgroud color, corner radius, shadow
- show images
- a toggle could be created using a box but maybe a checkbox need to be created
- an input text

The idea is that everything should be composed from these few component.

I want the library to be reactive so each props of these component should accept a fixed value, or a stream of values that should update only want is needed without recreating the whole tree.

It should be pretty so I would like to have an animation support using the hardware to optimize the performance.
