# Aspect ratios people can read

The relink dialogs show a photo's aspect ratio as width ÷ height to three places (`aspect_text` in `crates/app/src/app/relink.rs`). Show a ratio instead (4:3, 3:2, 16:9) when it's close to a common one, without odd ones like 1001:751 for a rounded resize.

Let's be careful about how this interacts with our relinking / matching algorithm; we shouldn't show the aspect ratios as different if they're within tolerances (for example) and therefore accept the image as the 'same' in that circumstance.

Also: we should probs enforce a minimum image size and ensure that rescaling won't crash the app if e.g. we start with a square image, transcribe it, and then resize down to a 1x1 pixel.
