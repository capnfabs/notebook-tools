# Aspect ratios people can read

The relink dialogs show a photo's aspect ratio as width ÷ height to three places (`aspect_text` in `crates/app/src/app/relink.rs`). Show a ratio instead (4:3, 3:2, 16:9) when it's close to a common one, without odd ones like 1001:751 for a rounded resize.
