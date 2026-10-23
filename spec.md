# notebook-tools

Notebook-tools is a tool for OCR-ing a handwritten notebook.

It needs a small, responsive desktop UI. We might port to mobile at some point. WASM is a bonus, but I suspect it might be difficult.

The basic flow we're working towards is:
- Allow the user to log into / connect the account to a Claude.ai account.
- Open a set of images taken by an iphone, probably in HEIC format.
- Automatically detect whether this is one page or a two-page spread.
- Align, transform and crop the images. Fix lighting so that they look as close to a scan as possible. I expect that we'll have to iterate on this a little.
- We'll send the images to Claude for transcription. The response we get back will _optionally_ include bounding boxes for uncertain words, and when you click on one of these bounding boxes, we'll offer alternatives OR the user can type their own. I've already got a bit of a design for how this should work.
- At the end, stitch it all into a searchable PDF (that is, have the text ), _and_ offer the ability to export to markdown.

Technology choices:

- Everything in Rust. I am ok revising this if it's going to make _other_ stuff hard.
- Help me pick a Gui framework.
- Try and work with abstractions for images as opposed to HEIC / JPEG files directly, wherever possible
