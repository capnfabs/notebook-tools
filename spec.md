# notebook-tools

Notebook-tools is a tool for OCR-ing a handwritten notebook.

It needs a small, responsive desktop UI. We might port to mobile at some point. WASM is a bonus, but I suspect it might be difficult.

The basic flow we're working towards is:
- Allow the user to connect the account to a Claude.ai account.
- Open a set of images taken by an iphone, probably in HEIC format.
- Automatically detect whether this is one page or a two-page spread.
- Align, transform and crop the images. Fix lighting so that they look as close to a scan as possible. I expect that we'll have to iterate on this a little.
- We'll send the images to Claude for transcription. The response we get back will _optionally_ include bounding boxes for uncertain words, and when you click on one of these bounding boxes, we'll offer alternatives OR the user can type their own. I've already got a bit of a design for how this should work.
- At the end, stitch it all into a searchable PDF (that is, have the text ), _and_ offer the ability to export to markdown.


## ui flow

- When a user opens the app, it shows a "drop image files here" message. The user can drag-and-drop files from their desktop.
- files are sorted by filename order by default, but can be rearranged w/ drag and drop.
- everything else is outlined in wireframes-206-10-23.excalidraw
