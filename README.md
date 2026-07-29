# hwboard

A local whiteboard app built with Rust + wgpu + SDL3. No cloud, just a lightweight pen-and-paper feel running on your own machine 🖊️

It's meant for jotting down math, quick diagrams, and explanations on the fly — so instead of chasing fancy art, I care a lot more about **accurate, smooth strokes**.

## What it can do

- Pressure-sensitive pen writing ✍️
- Eraser (precise hit-testing)
- Undo / Redo
- Paste images straight from the clipboard
- Select / move / resize / rotate
- Auto-snap to shapes when you pause mid-stroke (line, triangle, rectangle, circle)
- Infinite canvas (zoom / pan)

## Why it exists

Microsoft Whiteboard's personal-account service is getting shut down, so I figured I'd just build the small subset I actually need. That means no cloud sync or real-time collaboration on purpose — the focus is entirely on fast, accurate local drawing.

## Status

Actively a work in progress. Bug reports, ideas, and code contributions are all very welcome — feel free to open an issue!

## License

MIT — see the `LICENSE` file.