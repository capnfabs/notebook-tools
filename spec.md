# notebook-tools

Notebook-tools is a tool for OCR-ing a handwritten notebook.

It needs a small, responsive desktop UI. We might port to mobile at some point.

The basic flow we're working towards is:
- Open a set of images taken by an iphone, probably in HEIC format.
- Align, transform and crop the images. Fix lighting so that they look as close to a scan as possible.
- We'll send the images to Claude for transcription. The response we get back will _optionally_ include bounding boxes for uncertain words, and when you click on one of these bounding boxes, we'll offer alternatives OR the user can type their own.
- At the end, stitch it all into a searchable PDF (that is, have the text ), _and_ offer the ability to export to markdown.
