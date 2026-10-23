//! UI-agnostic logic for notebook-tools. Keep the public API to plain data types.

pub fn greeting(name: &str) -> String {
    format!("Hello, {name}!")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greets_by_name() {
        assert_eq!(greeting("notebook"), "Hello, notebook!");
    }
}
