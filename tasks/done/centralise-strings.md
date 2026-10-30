# Centralise UI strings

Move all user-facing English into one Fluent file (`i18n-embed-fl`, as in egui discussion #1751), so copy is easy to revise and i18n is possible later. Tests look strings up the same way. Core errors are rendered by the app.
